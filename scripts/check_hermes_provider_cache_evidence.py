#!/usr/bin/env python3
"""Fail-closed integrity check for committed D23 provider-cache evidence."""

from __future__ import annotations

from decimal import Decimal, ROUND_CEILING
import json
from pathlib import Path
import statistics
import sys
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_PATH = ROOT / "spikes" / "hermes" / "evidence" / "provider-cache.json"
PIN_PATH = ROOT / "spikes" / "hermes" / "pin.json"
MICRO_USD = Decimal("1000000")


class EvidenceError(ValueError):
    """Committed provider-cache evidence is incomplete or inconsistent."""


def exact_keys(document: dict[str, Any], expected: set[str], location: str) -> None:
    if set(document) != expected:
        raise EvidenceError(f"{location} schema differs")


def nonnegative_int(value: Any, location: str, *, positive: bool = False) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise EvidenceError(f"{location} must be a nonnegative integer")
    if positive and value == 0:
        raise EvidenceError(f"{location} must be positive")
    return value


def expected_cost(turn: dict[str, Any], prices: dict[str, Any]) -> int:
    buckets = (
        ("uncached_input_tokens", "uncached_input"),
        ("cached_input_tokens", "cached_input"),
        ("cache_write_input_tokens", "cache_write_input"),
        ("output_tokens", "output"),
    )
    cost = sum(
        Decimal(nonnegative_int(turn[token_name], token_name))
        * Decimal(nonnegative_int(prices[price_name], price_name))
        / MICRO_USD
        for token_name, price_name in buckets
    )
    return int(cost.to_integral_value(rounding=ROUND_CEILING))


def summarize(turns: list[dict[str, Any]]) -> dict[str, int]:
    return {
        "turns": len(turns),
        "uncached_input_tokens": sum(item["uncached_input_tokens"] for item in turns),
        "cached_input_tokens": sum(item["cached_input_tokens"] for item in turns),
        "cache_write_input_tokens": sum(
            item["cache_write_input_tokens"] for item in turns
        ),
        "output_tokens": sum(item["output_tokens"] for item in turns),
        "cost_micro_usd": sum(item["cost_micro_usd"] for item in turns),
        "latency_median_ms": round(
            statistics.median(item["latency_ms"] for item in turns)
        ),
    }


def validate_evidence(document: dict[str, Any], pin: dict[str, Any]) -> None:
    exact_keys(
        document,
        {
            "schema_version",
            "measurement",
            "pin",
            "boundary",
            "provider",
            "safeguards",
            "trial",
            "aggregate",
            "decision",
        },
        "root",
    )
    if document["schema_version"] != 2:
        raise EvidenceError("unsupported evidence schema version")
    if document["measurement"] != "direct_live_provider_byte_prefix_cache_control":
        raise EvidenceError("unexpected measurement")
    expected_pin = {
        "repository": pin["repository"],
        "tag": pin["tag"],
        "tag_object_sha": pin["tag_object_sha"],
        "commit_sha": pin["commit_sha"],
        "plan_source_commit_prefix": pin["plan_source_commit_prefix"],
        "package": pin["package"],
        "package_version": pin["package_version"],
        "python_requires": pin["python_requires"],
    }
    if document["pin"] != expected_pin:
        raise EvidenceError("evidence source identity differs from the SHA-256 pin")

    if document["boundary"] != {
        "request_path": "direct_provider_api",
        "hermes_in_request_path": False,
        "hermes_source_pin_verified": True,
        "hermes_http_cache_continuity_measured": False,
        "latency_interpretation": "uncontrolled_fixed_order_samples_only",
    }:
        raise EvidenceError("measurement boundary differs")

    provider = document["provider"]
    if not isinstance(provider, dict):
        raise EvidenceError("provider must be an object")
    exact_keys(
        provider,
        {
            "marketplace",
            "upstream_provider",
            "model",
            "wire_protocol",
            "price_updated_at",
            "price_micro_usd_per_million",
        },
        "provider",
    )
    if (
        provider["marketplace"] != "surplus_intelligence"
        or provider["upstream_provider"] != "anthropic"
        or provider["model"] != "claude-haiku-4.5"
        or provider["wire_protocol"] != "anthropic_messages"
    ):
        raise EvidenceError("provider identity differs from the reviewed trial")
    if not isinstance(provider["price_updated_at"], str) or not provider[
        "price_updated_at"
    ].endswith("Z"):
        raise EvidenceError("provider price timestamp is missing")
    prices = provider["price_micro_usd_per_million"]
    if not isinstance(prices, dict):
        raise EvidenceError("provider prices must be an object")
    exact_keys(
        prices,
        {"uncached_input", "cached_input", "cache_write_input", "output"},
        "provider prices",
    )
    for name, value in prices.items():
        nonnegative_int(value, f"provider price {name}", positive=True)

    safeguards = document["safeguards"]
    if not isinstance(safeguards, dict):
        raise EvidenceError("safeguards must be an object")
    exact_keys(
        safeguards,
        {
            "operator_opt_in_required",
            "environment_key_only",
            "hard_spend_ceiling_micro_usd",
            "planned_worst_case_micro_usd",
            "provider_pin_required",
            "raw_provider_output_persisted",
        },
        "safeguards",
    )
    if safeguards != {
        **safeguards,
        "operator_opt_in_required": True,
        "environment_key_only": "SURPLUS_API_KEY",
        "provider_pin_required": True,
        "raw_provider_output_persisted": False,
    }:
        raise EvidenceError("live-run safeguards differ")
    ceiling = nonnegative_int(
        safeguards["hard_spend_ceiling_micro_usd"], "hard spend ceiling", positive=True
    )
    planned = nonnegative_int(
        safeguards["planned_worst_case_micro_usd"], "planned worst case", positive=True
    )
    if ceiling > 3_000_000 or planned > ceiling:
        raise EvidenceError("spend ceiling contract differs")

    trial = document["trial"]
    if not isinstance(trial, dict):
        raise EvidenceError("trial must be an object")
    exact_keys(
        trial,
        {
            "turns_per_class",
            "prefix_bytes",
            "max_output_tokens",
            "pair_order",
            "cache_control_policy",
            "cold_definition",
            "long_lived_definition",
            "turns",
        },
        "trial",
    )
    turns_per_class = nonnegative_int(
        trial["turns_per_class"], "turns per class", positive=True
    )
    if not 2 <= turns_per_class <= 10 or trial["pair_order"] != "cold_then_long_lived":
        raise EvidenceError("reviewed paired-trial shape differs")
    if trial["cache_control_policy"] != (
        "ephemeral_cache_breakpoint_applied_to_both_classes"
    ):
        raise EvidenceError("cache-control comparison differs")
    prefix_bytes = nonnegative_int(trial["prefix_bytes"], "prefix bytes", positive=True)
    if prefix_bytes < 16_384:
        raise EvidenceError("controlled prefix is too short")
    max_output_tokens = nonnegative_int(
        trial["max_output_tokens"], "max output tokens", positive=True
    )
    if max_output_tokens > 32:
        raise EvidenceError("max output token bound differs")
    turns = trial["turns"]
    if not isinstance(turns, list) or len(turns) != turns_per_class:
        raise EvidenceError("paired turns are incomplete")
    turn_keys = {
        "uncached_input_tokens",
        "cached_input_tokens",
        "cache_write_input_tokens",
        "output_tokens",
        "cost_micro_usd",
        "latency_ms",
    }
    cold_turns: list[dict[str, Any]] = []
    warm_turns: list[dict[str, Any]] = []
    for ordinal, pair in enumerate(turns, start=1):
        if not isinstance(pair, dict):
            raise EvidenceError("paired turn must be an object")
        exact_keys(pair, {"ordinal", "cold_session", "long_lived_session"}, "pair")
        if pair["ordinal"] != ordinal:
            raise EvidenceError("turn ordinals must be contiguous")
        for class_name in ("cold_session", "long_lived_session"):
            turn = pair[class_name]
            if not isinstance(turn, dict):
                raise EvidenceError("turn aggregate must be an object")
            exact_keys(turn, turn_keys, f"turn {ordinal} {class_name}")
            for name, value in turn.items():
                nonnegative_int(value, f"turn {ordinal} {class_name} {name}")
            if turn["output_tokens"] > max_output_tokens:
                raise EvidenceError("turn output exceeds the configured bound")
            if turn["latency_ms"] == 0:
                raise EvidenceError("turn latency must be positive")
            prompt_tokens = (
                turn["uncached_input_tokens"]
                + turn["cached_input_tokens"]
                + turn["cache_write_input_tokens"]
            )
            if prompt_tokens < prefix_bytes // 16:
                raise EvidenceError("provider usage omits the controlled prompt")
            if turn["cost_micro_usd"] != expected_cost(turn, prices):
                raise EvidenceError("turn cost does not derive from provider usage")
        cold_turns.append(pair["cold_session"])
        warm_turns.append(pair["long_lived_session"])

    aggregate = document["aggregate"]
    if not isinstance(aggregate, dict):
        raise EvidenceError("aggregate must be an object")
    exact_keys(
        aggregate,
        {"cost_basis", "cold_sessions", "long_lived_session", "repeat_turns_only"},
        "aggregate",
    )
    if aggregate["cost_basis"] != (
        "provider usage buckets multiplied by the provider's published per-bucket rates"
    ):
        raise EvidenceError("cost basis differs")
    if aggregate["cold_sessions"] != summarize(cold_turns):
        raise EvidenceError("cold aggregate is not derived from turns")
    if aggregate["long_lived_session"] != summarize(warm_turns):
        raise EvidenceError("long-lived aggregate is not derived from turns")
    measured_cost = (
        aggregate["cold_sessions"]["cost_micro_usd"]
        + aggregate["long_lived_session"]["cost_micro_usd"]
    )
    if measured_cost > ceiling:
        raise EvidenceError("measured cost exceeds the hard spend ceiling")
    repeat = aggregate["repeat_turns_only"]
    if not isinstance(repeat, dict):
        raise EvidenceError("repeat aggregate must be an object")
    exact_keys(
        repeat,
        {
            "cold_cost_micro_usd",
            "long_lived_cost_micro_usd",
            "cost_saved_micro_usd",
            "cost_reduction_percent",
            "plain_uncached_comparator_cost_micro_usd",
            "plain_uncached_comparator_saved_micro_usd",
            "plain_uncached_comparator_reduction_percent",
        },
        "repeat aggregate",
    )
    cold_cost = sum(item["cost_micro_usd"] for item in cold_turns[1:])
    warm_cost = sum(item["cost_micro_usd"] for item in warm_turns[1:])
    saved = cold_cost - warm_cost
    reduction = round(saved * 10_000 / cold_cost) / 100 if cold_cost else 0.0
    plain_uncached_cost = sum(
        int(
            (
                Decimal(
                    item["uncached_input_tokens"]
                    + item["cached_input_tokens"]
                    + item["cache_write_input_tokens"]
                )
                * Decimal(prices["uncached_input"])
                / MICRO_USD
                + Decimal(item["output_tokens"])
                * Decimal(prices["output"])
                / MICRO_USD
            ).to_integral_value(rounding=ROUND_CEILING)
        )
        for item in cold_turns[1:]
    )
    plain_uncached_saved = plain_uncached_cost - warm_cost
    plain_uncached_reduction = (
        round(plain_uncached_saved * 10_000 / plain_uncached_cost) / 100
        if plain_uncached_cost
        else 0.0
    )
    if repeat != {
        "cold_cost_micro_usd": cold_cost,
        "long_lived_cost_micro_usd": warm_cost,
        "cost_saved_micro_usd": saved,
        "cost_reduction_percent": reduction,
        "plain_uncached_comparator_cost_micro_usd": plain_uncached_cost,
        "plain_uncached_comparator_saved_micro_usd": plain_uncached_saved,
        "plain_uncached_comparator_reduction_percent": plain_uncached_reduction,
    }:
        raise EvidenceError("repeat-turn savings are not derived")
    cache_preserved = (
        all(item["cached_input_tokens"] > 0 for item in warm_turns[1:])
        and sum(item["cached_input_tokens"] for item in warm_turns[1:])
        > sum(item["cached_input_tokens"] for item in cold_turns[1:])
    )
    if document["decision"] != {
        "provider_byte_prefix_cache_preserved": cache_preserved,
        "provider_cache_reduced_repeat_cost": saved > 0,
        "hermes_http_session_route_cache_continuity": "not_measured",
        "d23_http_session_route": "pending_end_to_end_hermes_measurement",
        "d24_multiplexing_evaluation": "independently_executable",
    }:
        raise EvidenceError("D23/D24 decision is not derived from the measured scope")


def main() -> int:
    try:
        raw = EVIDENCE_PATH.read_text(encoding="utf-8")
        for forbidden in (
            "inf_",
            "/Users/",
            "/home/",
            "Authorization",
            "x-api-key",
            "request_id",
            "assistant_text",
        ):
            if forbidden in raw:
                raise EvidenceError(f"evidence contains forbidden material: {forbidden}")
        document = json.loads(raw)
        pin = json.loads(PIN_PATH.read_text(encoding="utf-8"))
        if not isinstance(document, dict) or not isinstance(pin, dict):
            raise EvidenceError("evidence and pin must be JSON objects")
        validate_evidence(document, pin)
    except (OSError, EvidenceError, json.JSONDecodeError, KeyError) as error:
        print(f"Hermes provider-cache evidence: {error}", file=sys.stderr)
        return 1
    print("Hermes provider-cache evidence: valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
