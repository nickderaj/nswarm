#!/usr/bin/env python3
"""Tests for the committed D24 local-simulation evidence contract."""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "check_hermes_multiplex_evidence",
    ROOT / "scripts" / "check_hermes_multiplex_evidence.py",
)
assert SPEC is not None and SPEC.loader is not None
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class EvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.document = json.loads(CHECK.EVIDENCE_PATH.read_text(encoding="utf-8"))
        cls.pin = json.loads(CHECK.PIN_PATH.read_text(encoding="utf-8"))

    def test_committed_evidence_is_valid(self) -> None:
        CHECK.validate_evidence(copy.deepcopy(self.document), self.pin)

    def test_rejects_source_identity_drift(self) -> None:
        changed = copy.deepcopy(self.document)
        changed["pin"]["commit_sha"] = "0" * 40
        with self.assertRaisesRegex(CHECK.EvidenceError, "source identity"):
            CHECK.validate_evidence(changed, self.pin)

    def test_rejects_incomplete_test_aggregate(self) -> None:
        changed = copy.deepcopy(self.document)
        changed["trial"]["tests_passed"] -= 1
        with self.assertRaisesRegex(CHECK.EvidenceError, "passing-test totals"):
            CHECK.validate_evidence(changed, self.pin)

    def test_rejects_unmeasured_target_pi_or_pilot_claim(self) -> None:
        changed = copy.deepcopy(self.document)
        changed["environment"]["target_pi"] = True
        with self.assertRaisesRegex(CHECK.EvidenceError, "environment schema"):
            CHECK.validate_evidence(changed, self.pin)

        changed = copy.deepcopy(self.document)
        changed["decision"]["live_pilot"] = True
        with self.assertRaisesRegex(CHECK.EvidenceError, "decision is not derived"):
            CHECK.validate_evidence(changed, self.pin)

    def test_rejects_raw_or_secret_material(self) -> None:
        changed = copy.deepcopy(self.document)
        changed["environment"]["os"] = "/Users/operator/raw-output"
        with self.assertRaisesRegex(CHECK.EvidenceError, "raw or secret"):
            CHECK.validate_evidence(changed, self.pin)

    def test_internally_consistent_failed_result_is_valid_evidence(self) -> None:
        changed = copy.deepcopy(self.document)
        contract = changed["contracts"]["profile_route_and_allowlist"]
        contract["tests_passed"] -= 1
        contract["tests_failed"] = 1
        contract["result"] = "failed"
        changed["trial"]["tests_passed"] -= 1
        changed["trial"]["tests_failed"] = 1
        changed["decision"]["upstream_regression_suite"] = "failed"
        CHECK.validate_evidence(changed, self.pin)


if __name__ == "__main__":
    unittest.main()
