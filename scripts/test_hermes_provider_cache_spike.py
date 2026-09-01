#!/usr/bin/env python3
"""Tests for the cost-bounded Hermes provider-cache harness."""

from __future__ import annotations

from argparse import Namespace
from decimal import Decimal
import importlib.util
import os
from pathlib import Path
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


if __name__ == "__main__":
    unittest.main()
