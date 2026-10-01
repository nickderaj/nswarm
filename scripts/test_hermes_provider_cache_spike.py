#!/usr/bin/env python3
"""Tests for the cost-bounded Hermes provider-cache harness."""

from __future__ import annotations

from argparse import Namespace
import copy
from decimal import Decimal
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "hermes_provider_cache_spike",
    ROOT / "scripts" / "hermes_provider_cache_spike.py",
)
assert SPEC is not None and SPEC.loader is not None
SPIKE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SPIKE
SPEC.loader.exec_module(SPIKE)
CHECK_SPEC = importlib.util.spec_from_file_location(
    "check_hermes_provider_cache_evidence",
    ROOT / "scripts" / "check_hermes_provider_cache_evidence.py",
)
assert CHECK_SPEC is not None and CHECK_SPEC.loader is not None
CHECK = importlib.util.module_from_spec(CHECK_SPEC)
CHECK_SPEC.loader.exec_module(CHECK)


def args(source: Path, output: Path, **overrides: object) -> Namespace:
    values: dict[str, object] = {
        "source": source,
        "output": output,
        "turns": 4,
        "model": SPIKE.DEFAULT_MODEL,
        "provider": SPIKE.DEFAULT_PROVIDER,
        "prefix_bytes": 24_576,
        "max_output_tokens": 8,
        "max_spend_usd": "0.50",
        "i_understand_this_spends_money": True,
    }
    values.update(overrides)
    return Namespace(**values)


class LiveSafetyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.source = ROOT
        self.output = (
            ROOT / "spikes" / "hermes" / "evidence" / "provider-cache.json"
        )

    def test_requires_explicit_operator_opt_in(self) -> None:
        with (
            patch.dict(os.environ, {SPIKE.API_KEY_ENV: "inf_synthetic"}),
            self.assertRaisesRegex(SPIKE.ProviderCacheError, "spends-money"),
        ):
            SPIKE.validate_live_args(
                args(self.source, self.output, i_understand_this_spends_money=False)
            )

    def test_requires_environment_key(self) -> None:
        with (
            patch.dict(os.environ, {}, clear=True),
            self.assertRaisesRegex(SPIKE.ProviderCacheError, SPIKE.API_KEY_ENV),
        ):
            SPIKE.validate_live_args(args(self.source, self.output))

    def test_rejects_excessive_spend_ceiling(self) -> None:
        with (
            patch.dict(os.environ, {SPIKE.API_KEY_ENV: "inf_synthetic"}),
            self.assertRaisesRegex(SPIKE.ProviderCacheError, "no more than"),
        ):
            SPIKE.validate_live_args(
                args(self.source, self.output, max_spend_usd="3.000001")
            )

    def test_rejects_output_outside_aggregate_evidence_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with (
                patch.dict(os.environ, {SPIKE.API_KEY_ENV: "inf_synthetic"}),
                self.assertRaisesRegex(
                    SPIKE.ProviderCacheError, "spikes/hermes/evidence"
                ),
            ):
                SPIKE.validate_live_args(
                    args(self.source, Path(directory) / "raw-provider.json")
                )

    def test_accepts_bounded_configuration_without_retaining_key(self) -> None:
        with patch.dict(os.environ, {SPIKE.API_KEY_ENV: "inf_synthetic"}):
            config = SPIKE.validate_live_args(args(self.source, self.output))
        self.assertEqual(config.max_spend_micro_usd, 500_000)
        self.assertNotIn("key", config.__dataclass_fields__)

    def test_reservation_uses_uncached_or_cache_write_worst_case(self) -> None:
        reserved = SPIKE.reserve_micro_usd(
            request_bytes=24_000,
            max_output_tokens=8,
            input_price_per_million=Decimal("1"),
            cache_write_price_per_million=Decimal("1.25"),
            output_price_per_million=Decimal("5"),
        )
        self.assertEqual(reserved, 30_040)


class ProviderContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.config = SPIKE.LiveConfig(
            source=ROOT,
            output=(
                ROOT
                / "spikes"
                / "hermes"
                / "evidence"
                / "provider-cache.json"
            ),
            turns=3,
            model=SPIKE.DEFAULT_MODEL,
            provider=SPIKE.DEFAULT_PROVIDER,
            prefix_bytes=24_576,
            max_output_tokens=8,
            max_spend_usd=Decimal("1.00"),
        )
        self.quote = SPIKE.PriceQuote(
            input_per_million=Decimal("1"),
            output_per_million=Decimal("5"),
            cache_read_per_million=Decimal("0.1"),
            cache_write_per_million=Decimal("1.25"),
        )

    def test_price_quote_accepts_live_list_schema(self) -> None:
        response = {
            "models": [
                {
                    "model": SPIKE.DEFAULT_MODEL,
                    "providers": [
                        {
                            "provider": SPIKE.DEFAULT_PROVIDER,
                            "pricing": {
                                "input": 1,
                                "output": 5,
                                "cacheRead": 0.1,
                                "cacheWrite": 1.25,
                            },
                        }
                    ],
                }
            ],
            "updated_at": "2026-09-01T08:09:59.006Z",
        }
        with patch.object(SPIKE, "_request_json", return_value=(response, 1)):
            quote, updated_at = SPIKE.fetch_price_quote(
                SPIKE.DEFAULT_MODEL, SPIKE.DEFAULT_PROVIDER
            )
        self.assertEqual(quote, self.quote)
        self.assertEqual(updated_at, response["updated_at"])

    def test_price_quote_fails_closed_without_cache_write_rate(self) -> None:
        response = {
            "models": [
                {
                    "model": SPIKE.DEFAULT_MODEL,
                    "providers": [
                        {
                            "provider": SPIKE.DEFAULT_PROVIDER,
                            "pricing": {"input": 1, "output": 5, "cacheRead": 0.1},
                        }
                    ],
                }
            ],
            "updated_at": "2026-09-01T08:09:59.006Z",
        }
        with (
            patch.object(SPIKE, "_request_json", return_value=(response, 1)),
            self.assertRaisesRegex(SPIKE.ProviderCacheError, "cacheWrite/cache_write"),
        ):
            SPIKE.fetch_price_quote(SPIKE.DEFAULT_MODEL, SPIKE.DEFAULT_PROVIDER)

    def test_usage_cost_uses_all_provider_cache_buckets(self) -> None:
        usage = {
            "input_tokens": 100,
            "cache_read_input_tokens": 1_000,
            "cache_creation_input_tokens": 2_000,
            "output_tokens": 3,
        }
        self.assertEqual(SPIKE.usage_cost_micro_usd(usage, self.quote), 2_715)

    def test_invoke_discards_raw_response_metadata(self) -> None:
        response = {
            "id": "must-not-be-persisted",
            "content": [{"type": "text", "text": "ACK0001"}],
            "usage": {
                "input_tokens": 600,
                "cache_read_input_tokens": 1_000,
                "cache_creation_input_tokens": 0,
                "output_tokens": 2,
            },
        }
        with patch.object(SPIKE, "_request_json", return_value=(response, 321)):
            result = SPIKE.invoke_provider(
                api_key="inf_synthetic",
                config=self.config,
                quote=self.quote,
                system_prompt="safe",
                messages=[{"role": "user", "content": "safe"}],
            )
        self.assertEqual(result.cached_input_tokens, 1_000)
        self.assertEqual(result.latency_ms, 321)
        self.assertNotIn("id", result.__dataclass_fields__)

    def test_invoke_rejects_usage_that_omits_controlled_prefix(self) -> None:
        response = {
            "content": [{"type": "text", "text": "ACK0001"}],
            "usage": {
                "input_tokens": 12,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0,
                "output_tokens": 2,
            },
        }
        with (
            patch.object(SPIKE, "_request_json", return_value=(response, 321)),
            self.assertRaisesRegex(SPIKE.ProviderCacheError, "omitted too much"),
        ):
            SPIKE.invoke_provider(
                api_key="inf_synthetic",
                config=self.config,
                quote=self.quote,
                system_prompt="safe",
                messages=[{"role": "user", "content": "safe"}],
            )

    def test_system_prompts_are_equal_length_and_nonce_scoped(self) -> None:
        warm = SPIKE.make_system_prompt(
            24_576, "warm-session-prefix-000000000000"
        )
        cold = SPIKE.make_system_prompt(
            24_576, "cold-session-prefix-0001-0000000"
        )
        self.assertEqual(len(warm.encode()), len(cold.encode()))
        self.assertNotEqual(warm, cold)

    def test_measurement_fails_before_inference_when_reserve_exceeds_cap(self) -> None:
        config = SPIKE.LiveConfig(
            **{
                **self.config.__dict__,
                "max_spend_usd": Decimal("0.000001"),
            }
        )
        with (
            patch.object(
                SPIKE, "verify_pinned_source", return_value={"commit_sha": "a" * 40}
            ),
            patch.object(
                SPIKE,
                "fetch_price_quote",
                return_value=(self.quote, "2026-09-01T08:09:59.006Z"),
            ),
            patch.object(SPIKE, "invoke_provider") as invoke,
            self.assertRaisesRegex(SPIKE.ProviderCacheError, "worst-case spend"),
        ):
            SPIKE.measure_live(config)
        invoke.assert_not_called()

    def test_measurement_persists_only_aggregate_turn_fields(self) -> None:
        cold = SPIKE.TurnUsage(100, 0, 2_000, 2, 2_510, 500, "cold output")
        warm_prime = SPIKE.TurnUsage(100, 0, 2_000, 2, 2_510, 450, "warm one")
        warm_hit = SPIKE.TurnUsage(100, 2_000, 0, 2, 310, 200, "warm repeat")
        side_effect = [cold, warm_prime, cold, warm_hit, cold, warm_hit]
        with (
            patch.dict(os.environ, {SPIKE.API_KEY_ENV: "inf_synthetic"}),
            patch.object(
                SPIKE, "verify_pinned_source", return_value={"commit_sha": "a" * 40}
            ),
            patch.object(
                SPIKE,
                "fetch_price_quote",
                return_value=(self.quote, "2026-09-01T08:09:59.006Z"),
            ),
            patch.object(
                SPIKE,
                "secrets",
                **{"token_hex.return_value": "a" * 32},
            ),
            patch.object(SPIKE, "invoke_provider", side_effect=side_effect),
            patch.object(SPIKE, "_write_partial_checkpoint"),
        ):
            evidence = SPIKE.measure_live(self.config)
        rendered = str(evidence)
        self.assertNotIn("cold output", rendered)
        self.assertNotIn("warm repeat", rendered)
        self.assertNotIn("request_id", rendered)
        self.assertTrue(
            evidence["decision"]["provider_byte_prefix_cache_preserved"]
        )
        self.assertEqual(evidence["boundary"]["hermes_in_request_path"], False)
        self.assertEqual(
            evidence["decision"]["d23_http_session_route"],
            "pending_end_to_end_hermes_measurement",
        )

    def test_partial_checkpoint_contains_only_count_and_cost_aggregates(self) -> None:
        usage = SPIKE.TurnUsage(100, 0, 2_000, 2, 2_510, 500, "raw output")
        with tempfile.TemporaryDirectory() as directory:
            config = SPIKE.LiveConfig(
                **{
                    **self.config.__dict__,
                    "output": Path(directory) / "provider-cache.json",
                }
            )
            SPIKE._write_partial_checkpoint(
                config,
                {"commit_sha": "a" * 40},
                [usage],
                [],
            )
            rendered = SPIKE._partial_path(config.output).read_text(encoding="utf-8")
        self.assertNotIn("raw output", rendered)
        checkpoint = json.loads(rendered)
        self.assertEqual(checkpoint["completed_requests"], 1)
        self.assertEqual(
            checkpoint["aggregate"]["cold_sessions"]["cost_micro_usd"], 2_510
        )

    def test_main_catches_source_verification_timeout(self) -> None:
        timeout = subprocess.TimeoutExpired("verify", 30)
        with (
            patch.object(SPIKE, "validate_live_args", side_effect=timeout),
            patch.object(
                sys,
                "argv",
                [
                    "hermes_provider_cache_spike.py",
                    "--source",
                    str(ROOT),
                    "--output",
                    str(self.config.output),
                    "--max-spend-usd",
                    "0.50",
                ],
            ),
        ):
            self.assertEqual(SPIKE.main(), 1)


class EvidenceIntegrityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.evidence = json.loads(CHECK.EVIDENCE_PATH.read_text(encoding="utf-8"))
        cls.pin = json.loads(CHECK.PIN_PATH.read_text(encoding="utf-8"))

    def test_committed_evidence_is_derived_and_complete(self) -> None:
        CHECK.validate_evidence(copy.deepcopy(self.evidence), self.pin)

    def test_tampered_turn_cost_fails_closed(self) -> None:
        evidence = copy.deepcopy(self.evidence)
        evidence["trial"]["turns"][1]["long_lived_session"][
            "cost_micro_usd"
        ] += 1
        with self.assertRaisesRegex(CHECK.EvidenceError, "turn cost"):
            CHECK.validate_evidence(evidence, self.pin)

    def test_tampered_aggregate_fails_closed(self) -> None:
        evidence = copy.deepcopy(self.evidence)
        evidence["aggregate"]["cold_sessions"]["cached_input_tokens"] = 1
        with self.assertRaisesRegex(CHECK.EvidenceError, "cold aggregate"):
            CHECK.validate_evidence(evidence, self.pin)

    def test_internally_consistent_adverse_result_is_valid(self) -> None:
        evidence = copy.deepcopy(self.evidence)
        prices = evidence["provider"]["price_micro_usd_per_million"]
        for pair in evidence["trial"]["turns"][1:]:
            warm = pair["long_lived_session"]
            warm["uncached_input_tokens"] = 9_000
            warm["cached_input_tokens"] = 0
            warm["cache_write_input_tokens"] = 0
            warm["cost_micro_usd"] = CHECK.expected_cost(warm, prices)
        cold = [pair["cold_session"] for pair in evidence["trial"]["turns"]]
        warm = [pair["long_lived_session"] for pair in evidence["trial"]["turns"]]
        evidence["aggregate"]["cold_sessions"] = CHECK.summarize(cold)
        evidence["aggregate"]["long_lived_session"] = CHECK.summarize(warm)
        cold_cost = sum(item["cost_micro_usd"] for item in cold[1:])
        warm_cost = sum(item["cost_micro_usd"] for item in warm[1:])
        saved = cold_cost - warm_cost
        plain_cost = evidence["aggregate"]["repeat_turns_only"][
            "plain_uncached_comparator_cost_micro_usd"
        ]
        repeat = evidence["aggregate"]["repeat_turns_only"]
        repeat.update(
            {
                "cold_cost_micro_usd": cold_cost,
                "long_lived_cost_micro_usd": warm_cost,
                "cost_saved_micro_usd": saved,
                "cost_reduction_percent": round(saved * 10_000 / cold_cost) / 100,
                "plain_uncached_comparator_saved_micro_usd": plain_cost - warm_cost,
                "plain_uncached_comparator_reduction_percent": round(
                    (plain_cost - warm_cost) * 10_000 / plain_cost
                )
                / 100,
            }
        )
        evidence["decision"]["provider_byte_prefix_cache_preserved"] = False
        evidence["decision"]["provider_cache_reduced_repeat_cost"] = saved > 0
        CHECK.validate_evidence(evidence, self.pin)


if __name__ == "__main__":
    unittest.main()
