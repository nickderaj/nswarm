#!/usr/bin/env python3
"""Cost-bounded live-provider prefix-cache measurement for Hermes D23."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation, ROUND_CEILING
import json
import os
from pathlib import Path
import sys
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_ROOT = (ROOT / "spikes" / "hermes" / "evidence").resolve()
OPT_IN_FLAG = "--i-understand-this-spends-money"
API_KEY_ENV = "SURPLUS_API_KEY"
DEFAULT_MODEL = "claude-haiku-4.5"
DEFAULT_PROVIDER = "bankr"
MAX_TURNS = 10
MAX_SPEND_CEILING_USD = Decimal("3.00")
MICRO_USD = Decimal("1000000")


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


def measure_live(config: LiveConfig) -> dict[str, Any]:
    """Run the live experiment after all preflight gates have passed."""
    raise ProviderCacheError("live provider implementation is not installed yet")


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
