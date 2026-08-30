#!/usr/bin/env python3
"""Tests for the pinned Hermes source-contract harness."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "hermes_gateway_spike", ROOT / "scripts" / "hermes_gateway_spike.py"
)
assert SPEC is not None and SPEC.loader is not None
SPIKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SPIKE)


class PinTests(unittest.TestCase):
    def test_committed_pin_is_complete_and_immutable(self) -> None:
        pin = SPIKE.load_pin()
        self.assertEqual(pin["tag"], "v2026.8.19")
        self.assertEqual(pin["package_version"], "0.20.5")
        self.assertEqual(pin["python_requires"], ">=3.11,<3.14")
        self.assertEqual(len(pin["source_files"]), 7)
        self.assertTrue(all(len(digest) == 64 for digest in pin["source_files"].values()))

    def test_pin_parser_rejects_unknown_fields(self) -> None:
        document = json.loads(SPIKE.PIN_PATH.read_text(encoding="utf-8"))
        document["floating_ref"] = "main"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "pin.json"
            path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaisesRegex(SPIKE.SpikeError, "schema differs"):
                SPIKE.load_pin(path)


class AstContractTests(unittest.TestCase):
    def test_call_finder_counts_only_direct_owner_calls(self) -> None:
        tree = SPIKE.ast.parse(
            "def run(self, other):\n"
            "    self._create_agent()\n"
            "    other._create_agent()\n"
        )
        node = SPIKE.function(tree, "run")
        self.assertEqual(SPIKE.calls_attribute(node, "self", "_create_agent"), [2])

    def test_missing_method_fails_closed(self) -> None:
        tree = SPIKE.ast.parse("class Adapter:\n    pass\n")
        with self.assertRaisesRegex(SPIKE.SpikeError, "missing Adapter.chat"):
            SPIKE.class_method(tree, "Adapter", "chat")


if __name__ == "__main__":
    unittest.main()
