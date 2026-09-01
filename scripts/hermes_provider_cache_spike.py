#!/usr/bin/env python3
"""Cost-bounded live-provider prefix-cache measurement for Hermes D23."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation, ROUND_CEILING
import json
import os
from pathlib import Path
import secrets
import statistics
import subprocess
import sys
import time
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_ROOT = (ROOT / "spikes" / "hermes" / "evidence").resolve()
OPT_IN_FLAG = "--i-understand-this-spends-money"
API_KEY_ENV = "SURPLUS_API_KEY"
DEFAULT_MODEL = "claude-haiku-4.5"
DEFAULT_PROVIDER = "anthropic"
MAX_TURNS = 10
MAX_SPEND_CEILING_USD = Decimal("3.00")
MICRO_USD = Decimal("1000000")
API_ORIGIN = "https://api.surplusintelligence.ai"
PRICE_PATH = "/v1/prices"
MESSAGES_PATH = "/anthropic/v1/messages"
REQUEST_TIMEOUT_SECONDS = 90
REQUEST_ENVELOPE_RESERVE_BYTES = 4_096


class ProviderCacheError(ValueError):
    """A live-measurement safety or evidence contract did not match."""


@dataclass(frozen=True)
class LiveConfig:
    source: Path
    output: Path
    turns: int
    model: str
    provider: str
    prefix_bytes: int
    max_output_tokens: int
    max_spend_usd: Decimal

    @property
    def max_spend_micro_usd(self) -> int:
        return int((self.max_spend_usd * MICRO_USD).to_integral_exact())


@dataclass(frozen=True)
class PriceQuote:
    input_per_million: Decimal
    output_per_million: Decimal
    cache_read_per_million: Decimal
    cache_write_per_million: Decimal


@dataclass(frozen=True)
class TurnUsage:
    uncached_input_tokens: int
    cached_input_tokens: int
    cache_write_input_tokens: int
    output_tokens: int
    cost_micro_usd: int
    latency_ms: int
    assistant_text: str


def _decimal(value: str, name: str) -> Decimal:
    try:
        parsed = Decimal(value)
    except InvalidOperation as error:
        raise ProviderCacheError(f"{name} must be a decimal number") from error
    if not parsed.is_finite():
        raise ProviderCacheError(f"{name} must be finite")
    return parsed


def _evidence_path(path: Path) -> Path:
    resolved = path.resolve()
    try:
        resolved.relative_to(EVIDENCE_ROOT)
    except ValueError as error:
        raise ProviderCacheError(
            "aggregate evidence output must stay under spikes/hermes/evidence"
        ) from error
    if resolved.suffix != ".json":
        raise ProviderCacheError("aggregate evidence output must be a JSON file")
    return resolved


def validate_live_args(args: argparse.Namespace) -> LiveConfig:
    if not args.i_understand_this_spends_money:
        raise ProviderCacheError(f"live measurement requires {OPT_IN_FLAG}")
    key = os.environ.get(API_KEY_ENV, "")
    if not key:
        raise ProviderCacheError(f"live measurement requires {API_KEY_ENV}")
    if not key.startswith("inf_") or any(character.isspace() for character in key):
        raise ProviderCacheError(f"{API_KEY_ENV} does not match the expected key shape")
    if args.turns < 2 or args.turns > MAX_TURNS:
        raise ProviderCacheError(f"turns must be between 2 and {MAX_TURNS}")
    if args.prefix_bytes < 16_384 or args.prefix_bytes > 65_536:
        raise ProviderCacheError("prefix-bytes must be between 16384 and 65536")
    if args.max_output_tokens < 1 or args.max_output_tokens > 32:
        raise ProviderCacheError("max-output-tokens must be between 1 and 32")
    ceiling = _decimal(args.max_spend_usd, "max-spend-usd")
    if ceiling <= 0 or ceiling > MAX_SPEND_CEILING_USD:
        raise ProviderCacheError(
            f"max-spend-usd must be positive and no more than {MAX_SPEND_CEILING_USD}"
        )
    if ceiling.as_tuple().exponent < -6:
        raise ProviderCacheError("max-spend-usd supports at most six decimal places")
    source = args.source.resolve()
    if not source.is_dir():
        raise ProviderCacheError("pinned Hermes source is not a directory")
    return LiveConfig(
        source=source,
        output=_evidence_path(args.output),
        turns=args.turns,
        model=args.model,
        provider=args.provider,
        prefix_bytes=args.prefix_bytes,
        max_output_tokens=args.max_output_tokens,
        max_spend_usd=ceiling,
    )


def reserve_micro_usd(
    *,
    request_bytes: int,
    max_output_tokens: int,
    input_price_per_million: Decimal,
    cache_write_price_per_million: Decimal,
    output_price_per_million: Decimal,
) -> int:
    """Conservatively reserve one token per input byte plus output capacity."""
    if request_bytes <= 0 or max_output_tokens <= 0:
        raise ProviderCacheError("reservation sizes must be positive")
    prices = (
        input_price_per_million,
        cache_write_price_per_million,
        output_price_per_million,
    )
    if any(price < 0 or not price.is_finite() for price in prices):
        raise ProviderCacheError("reservation prices must be finite and nonnegative")
    uncached_rate = max(input_price_per_million, cache_write_price_per_million)
    dollars = (
        Decimal(request_bytes) * uncached_rate
        + Decimal(max_output_tokens) * output_price_per_million
    ) / MICRO_USD
    return int((dollars * MICRO_USD).to_integral_value(rounding=ROUND_CEILING))


def verify_pinned_source(source: Path) -> dict[str, Any]:
    command = [
        sys.executable,
        str(ROOT / "scripts" / "hermes_gateway_spike.py"),
        "verify-source",
        "--source",
        str(source),
    ]
    result = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or "pinned source verification failed"
        raise ProviderCacheError(detail)
    try:
        document = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ProviderCacheError("pinned source verifier returned invalid JSON") from error
    pin = document.get("pin")
    if not isinstance(pin, dict) or not pin.get("commit_sha"):
        raise ProviderCacheError("pinned source verifier omitted its identity")
    return pin


def _request_json(
    *,
    path: str,
    api_key: str | None = None,
    body: dict[str, Any] | None = None,
    query: dict[str, str] | None = None,
) -> tuple[dict[str, Any], int]:
    url = API_ORIGIN + path
    if query:
        url += "?" + urlencode(query)
    headers = {"Accept": "application/json", "User-Agent": "nswarm-d23-spike/1"}
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body, separators=(",", ":")).encode("utf-8")
    if api_key is not None:
        headers["Authorization"] = f"Bearer {api_key}"
        headers["x-api-key"] = api_key
        headers["anthropic-version"] = "2023-06-01"
    request = Request(url, data=data, headers=headers, method="POST" if data else "GET")
    started = time.perf_counter_ns()
    try:
        with urlopen(request, timeout=REQUEST_TIMEOUT_SECONDS) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except HTTPError as error:
        raise ProviderCacheError(f"provider request failed with HTTP {error.code}") from error
    except URLError as error:
        raise ProviderCacheError("provider request failed before receiving a response") from error
    except json.JSONDecodeError as error:
        raise ProviderCacheError("provider returned invalid JSON") from error
    latency_ms = round((time.perf_counter_ns() - started) / 1_000_000)
    if not isinstance(payload, dict):
        raise ProviderCacheError("provider response must be a JSON object")
    return payload, latency_ms


def _price_decimal(pricing: dict[str, Any], *names: str) -> Decimal:
    for name in names:
        if name in pricing and pricing[name] is not None:
            value = _decimal(str(pricing[name]), f"provider price {name}")
            if value < 0:
                raise ProviderCacheError("provider prices must be nonnegative")
            return value
    raise ProviderCacheError(f"provider price is missing ({'/'.join(names)})")


def fetch_price_quote(model: str, provider: str) -> tuple[PriceQuote, str]:
    document, _ = _request_json(path=PRICE_PATH, query={"model": model})
    models = document.get("models")
    if not isinstance(models, list):
        raise ProviderCacheError("price response omitted models")
    matches = [item for item in models if isinstance(item, dict) and item.get("model") == model]
    if len(matches) != 1:
        raise ProviderCacheError("price response did not identify exactly one requested model")
    providers = matches[0].get("providers")
    pricing: dict[str, Any] | None = None
    if isinstance(providers, list):
        provider_matches = [
            item
            for item in providers
            if isinstance(item, dict) and item.get("provider") == provider
        ]
        if len(provider_matches) == 1 and isinstance(provider_matches[0].get("pricing"), dict):
            pricing = provider_matches[0]["pricing"]
    elif isinstance(providers, dict) and isinstance(providers.get(provider), dict):
        pricing = providers[provider]
    if pricing is None:
        raise ProviderCacheError("requested provider has no published model price")
    quote = PriceQuote(
        input_per_million=_price_decimal(pricing, "input"),
        output_per_million=_price_decimal(pricing, "output"),
        cache_read_per_million=_price_decimal(pricing, "cacheRead", "cache_read"),
        cache_write_per_million=_price_decimal(
            pricing, "cacheWrite", "cache_write", "input"
        ),
    )
    updated_at = document.get("updated_at")
    if not isinstance(updated_at, str) or not updated_at:
        raise ProviderCacheError("price response omitted updated_at")
    return quote, updated_at


def _usage_int(usage: dict[str, Any], name: str, *, required: bool = False) -> int:
    value = usage.get(name)
    if value is None and not required:
        return 0
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ProviderCacheError(f"provider usage {name} must be a nonnegative integer")
    return value


def usage_cost_micro_usd(usage: dict[str, Any], quote: PriceQuote) -> int:
    uncached = _usage_int(usage, "input_tokens", required=True)
    cached = _usage_int(usage, "cache_read_input_tokens")
    written = _usage_int(usage, "cache_creation_input_tokens")
    output = _usage_int(usage, "output_tokens", required=True)
    cost = (
        Decimal(uncached) * quote.input_per_million
        + Decimal(cached) * quote.cache_read_per_million
        + Decimal(written) * quote.cache_write_per_million
        + Decimal(output) * quote.output_per_million
    )
    return int(cost.to_integral_value(rounding=ROUND_CEILING))


def _assistant_text(document: dict[str, Any]) -> str:
    content = document.get("content")
    if not isinstance(content, list):
        raise ProviderCacheError("provider response omitted content")
    parts = [
        item.get("text")
        for item in content
        if isinstance(item, dict) and item.get("type") == "text"
    ]
    if not parts or not all(isinstance(part, str) for part in parts):
        raise ProviderCacheError("provider response omitted assistant text")
    return "".join(parts)


def invoke_provider(
    *,
    api_key: str,
    config: LiveConfig,
    quote: PriceQuote,
    system_prompt: str,
    messages: list[dict[str, str]],
) -> TurnUsage:
    document, latency_ms = _request_json(
        path=MESSAGES_PATH,
        api_key=api_key,
        body={
            "model": config.model,
            "provider": config.provider,
            "max_tokens": config.max_output_tokens,
            "temperature": 0,
            "max_price_per_1m": float(quote.input_per_million),
            "system": [
                {
                    "type": "text",
                    "text": system_prompt,
                    "cache_control": {"type": "ephemeral"},
                }
            ],
            "messages": messages,
        },
    )
    usage = document.get("usage")
    if not isinstance(usage, dict):
        raise ProviderCacheError("provider response omitted usage")
    uncached = _usage_int(usage, "input_tokens", required=True)
    cached = _usage_int(usage, "cache_read_input_tokens")
    written = _usage_int(usage, "cache_creation_input_tokens")
    minimum_prompt_tokens = config.prefix_bytes // 16
    if uncached + cached + written < minimum_prompt_tokens:
        raise ProviderCacheError(
            "provider usage omitted too much of the controlled prompt to measure cache behavior"
        )
    return TurnUsage(
        uncached_input_tokens=uncached,
        cached_input_tokens=cached,
        cache_write_input_tokens=written,
        output_tokens=_usage_int(usage, "output_tokens", required=True),
        cost_micro_usd=usage_cost_micro_usd(usage, quote),
        latency_ms=latency_ms,
        assistant_text=_assistant_text(document),
    )


def make_system_prompt(prefix_bytes: int, nonce: str) -> str:
    if len(nonce.encode("ascii")) != 32:
        raise ProviderCacheError("cache-scope nonce must be exactly 32 ASCII bytes")
    lead = (
        f"D23 cache-scope: {nonce}\n"
        "This is a controlled prompt-cache measurement. Reply to every user "
        "message with only ACK followed by its four decimal digits.\n"
    )
    filler = "Hermes persisted prompt continuity measurement block 0123456789.\n"
    if len(lead) > prefix_bytes:
        raise ProviderCacheError("prefix is too small for its fixed contract")
    prompt = (lead + filler * ((prefix_bytes - len(lead)) // len(filler) + 1))[
        :prefix_bytes
    ]
    if len(prompt.encode("ascii")) != prefix_bytes:
        raise ProviderCacheError("system prompt is not the requested byte length")
    return prompt


def _request_size_bound(config: LiveConfig, ordinal: int) -> int:
    user_bytes = sum(len(f"ACK request {index:04d}".encode()) for index in range(1, ordinal + 1))
    assistant_bytes = (ordinal - 1) * config.max_output_tokens * 8
    return config.prefix_bytes + user_bytes + assistant_bytes + REQUEST_ENVELOPE_RESERVE_BYTES


def planned_reserve_micro_usd(config: LiveConfig, quote: PriceQuote) -> int:
    return sum(
        2
        * reserve_micro_usd(
            request_bytes=_request_size_bound(config, ordinal),
            max_output_tokens=config.max_output_tokens,
            input_price_per_million=quote.input_per_million,
            cache_write_price_per_million=quote.cache_write_per_million,
            output_price_per_million=quote.output_per_million,
        )
        for ordinal in range(1, config.turns + 1)
    )


def _public_turn(usage: TurnUsage) -> dict[str, int]:
    return {
        "uncached_input_tokens": usage.uncached_input_tokens,
        "cached_input_tokens": usage.cached_input_tokens,
        "cache_write_input_tokens": usage.cache_write_input_tokens,
        "output_tokens": usage.output_tokens,
        "cost_micro_usd": usage.cost_micro_usd,
        "latency_ms": usage.latency_ms,
    }


def _summarize(turns: list[TurnUsage]) -> dict[str, int]:
    return {
        "turns": len(turns),
        "uncached_input_tokens": sum(item.uncached_input_tokens for item in turns),
        "cached_input_tokens": sum(item.cached_input_tokens for item in turns),
        "cache_write_input_tokens": sum(item.cache_write_input_tokens for item in turns),
        "output_tokens": sum(item.output_tokens for item in turns),
        "cost_micro_usd": sum(item.cost_micro_usd for item in turns),
        "latency_median_ms": round(statistics.median(item.latency_ms for item in turns)),
    }


def measure_live(config: LiveConfig) -> dict[str, Any]:
    """Run the live experiment after all preflight gates have passed."""
    pin = verify_pinned_source(config.source)
    quote, price_updated_at = fetch_price_quote(config.model, config.provider)
    planned_reserve = planned_reserve_micro_usd(config, quote)
    if planned_reserve > config.max_spend_micro_usd:
        raise ProviderCacheError(
            "planned worst-case spend exceeds the operator's hard spend ceiling"
        )
    api_key = os.environ[API_KEY_ENV]
    stable_prompt = make_system_prompt(config.prefix_bytes, secrets.token_hex(16))
    history: list[dict[str, str]] = []
    warm_turns: list[TurnUsage] = []
    cold_turns: list[TurnUsage] = []
    paired_turns: list[dict[str, Any]] = []
    actual_cost = 0
    for ordinal in range(1, config.turns + 1):
        user_text = f"ACK request {ordinal:04d}"
        messages = [*history, {"role": "user", "content": user_text}]
        cold_prompt = make_system_prompt(config.prefix_bytes, secrets.token_hex(16))
        cold = invoke_provider(
            api_key=api_key,
            config=config,
            quote=quote,
            system_prompt=cold_prompt,
            messages=messages,
        )
        actual_cost += cold.cost_micro_usd
        if actual_cost > config.max_spend_micro_usd:
            raise ProviderCacheError("provider-metered spend exceeded the hard ceiling")
        warm = invoke_provider(
            api_key=api_key,
            config=config,
            quote=quote,
            system_prompt=stable_prompt,
            messages=messages,
        )
        actual_cost += warm.cost_micro_usd
        if actual_cost > config.max_spend_micro_usd:
            raise ProviderCacheError("provider-metered spend exceeded the hard ceiling")
        cold_turns.append(cold)
        warm_turns.append(warm)
        paired_turns.append(
            {
                "ordinal": ordinal,
                "cold_session": _public_turn(cold),
                "long_lived_session": _public_turn(warm),
            }
        )
        history.extend(
            [
                {"role": "user", "content": user_text},
                {"role": "assistant", "content": warm.assistant_text},
            ]
        )
    cold_repeat = cold_turns[1:]
    warm_repeat = warm_turns[1:]
    cold_repeat_cost = sum(item.cost_micro_usd for item in cold_repeat)
    warm_repeat_cost = sum(item.cost_micro_usd for item in warm_repeat)
    saved_micro = cold_repeat_cost - warm_repeat_cost
    savings_percent = (
        round(saved_micro * 10_000 / cold_repeat_cost) / 100
        if cold_repeat_cost
        else 0.0
    )
    cache_preserved = (
        all(item.cached_input_tokens > 0 for item in warm_repeat)
        and sum(item.cached_input_tokens for item in warm_repeat)
        > sum(item.cached_input_tokens for item in cold_repeat)
    )
    return {
        "schema_version": 1,
        "measurement": "live_provider_byte_prefix_cache",
        "pin": pin,
        "provider": {
            "marketplace": "surplus_intelligence",
            "upstream_provider": config.provider,
            "model": config.model,
            "wire_protocol": "anthropic_messages",
            "price_updated_at": price_updated_at,
            "price_micro_usd_per_million": {
                "uncached_input": int(quote.input_per_million * MICRO_USD),
                "cached_input": int(quote.cache_read_per_million * MICRO_USD),
                "cache_write_input": int(quote.cache_write_per_million * MICRO_USD),
                "output": int(quote.output_per_million * MICRO_USD),
            },
        },
        "safeguards": {
            "operator_opt_in_required": True,
            "environment_key_only": API_KEY_ENV,
            "hard_spend_ceiling_micro_usd": config.max_spend_micro_usd,
            "planned_worst_case_micro_usd": planned_reserve,
            "provider_pin_required": True,
            "raw_provider_output_persisted": False,
        },
        "trial": {
            "turns_per_class": config.turns,
            "prefix_bytes": config.prefix_bytes,
            "max_output_tokens": config.max_output_tokens,
            "pair_order": "cold_then_long_lived",
            "cold_definition": "same-length transcript with a unique 32-byte nonce near the start of the persisted system prompt",
            "long_lived_definition": "growing transcript with one byte-identical persisted system prompt",
            "turns": paired_turns,
        },
        "aggregate": {
            "cost_basis": "provider usage buckets multiplied by the provider's published per-bucket rates",
            "cold_sessions": _summarize(cold_turns),
            "long_lived_session": _summarize(warm_turns),
            "repeat_turns_only": {
                "cold_cost_micro_usd": cold_repeat_cost,
                "long_lived_cost_micro_usd": warm_repeat_cost,
                "cost_saved_micro_usd": saved_micro,
                "cost_reduction_percent": savings_percent,
            },
        },
        "decision": {
            "provider_byte_prefix_cache_preserved": cache_preserved,
            "d23_http_session_route": "acceptable" if cache_preserved and saved_micro > 0 else "fallback_required",
            "d24_multiplexing_evaluation": "unblocked" if cache_preserved and saved_micro > 0 else "blocked",
        },
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--turns", type=int, default=4)
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--provider", default=DEFAULT_PROVIDER)
    parser.add_argument("--prefix-bytes", type=int, default=24_576)
    parser.add_argument("--max-output-tokens", type=int, default=8)
    parser.add_argument("--max-spend-usd", required=True)
    parser.add_argument(OPT_IN_FLAG, action="store_true")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        config = validate_live_args(args)
        result = measure_live(config)
        rendered = json.dumps(result, indent=2, sort_keys=True) + "\n"
        config.output.write_text(rendered, encoding="utf-8")
    except (OSError, ProviderCacheError, json.JSONDecodeError) as error:
        print(f"Hermes provider-cache spike: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
