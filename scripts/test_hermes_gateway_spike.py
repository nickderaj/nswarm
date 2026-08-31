#!/usr/bin/env python3
"""Tests for the pinned Hermes source-contract harness."""

from __future__ import annotations

import asyncio
import importlib.util
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import AsyncMock, patch


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
        self.assertEqual(
            set(pin["source_files"]),
            {
                "agent/agent_init.py",
                "agent/conversation_loop.py",
                "gateway/platforms/api_server.py",
                "gateway/platforms/base.py",
                "gateway/relay/adapter.py",
                "gateway/relay/transport.py",
                "gateway/relay/ws_transport.py",
                "gateway/run.py",
                "pyproject.toml",
                "tools/mcp_tool.py",
                "uv.lock",
            },
        )
        self.assertEqual(len(pin["capabilities_contract_sha256"]), 64)
        self.assertTrue(all(len(digest) == 64 for digest in pin["source_files"].values()))

    def test_pin_parser_rejects_unknown_fields(self) -> None:
        document = json.loads(SPIKE.PIN_PATH.read_text(encoding="utf-8"))
        document["floating_ref"] = "main"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "pin.json"
            path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaisesRegex(SPIKE.SpikeError, "schema differs"):
                SPIKE.load_pin(path)

    def test_pin_parser_rejects_invalid_plan_source_prefix(self) -> None:
        document = json.loads(SPIKE.PIN_PATH.read_text(encoding="utf-8"))
        document["plan_source_commit_prefix"] = "not-a-sha"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "pin.json"
            path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaisesRegex(SPIKE.SpikeError, "lowercase hexadecimal"):
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

    def test_annotated_tag_object_drift_fails_closed(self) -> None:
        pin = SPIKE.load_pin()
        with patch.object(
            SPIKE,
            "git_output",
            side_effect=[pin["commit_sha"], pin["tag"], "0" * 40],
        ):
            with self.assertRaisesRegex(SPIKE.SpikeError, "tag object differs"):
                SPIKE.verify_git_identity(Path("/source"), pin)

    def test_measurement_restores_sys_path_after_failure(self) -> None:
        original = sys.path.copy()
        with (
            patch.object(SPIKE, "load_pin", return_value={}),
            patch.object(SPIKE, "verify_source", return_value={}),
            patch.object(
                SPIKE,
                "_measure_http_reuse",
                new=AsyncMock(side_effect=RuntimeError("probe failure")),
            ),
            self.assertRaisesRegex(RuntimeError, "probe failure"),
        ):
            asyncio.run(SPIKE.measure_http_reuse(Path("/source"), 3))
        self.assertEqual(sys.path, original)


class EvidenceContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.evidence_path = (
            ROOT / "spikes" / "hermes" / "evidence" / "http-reuse.json"
        )
        cls.evidence = json.loads(cls.evidence_path.read_text(encoding="utf-8"))

    def test_capabilities_match_exact_pinned_contract(self) -> None:
        canonical = json.dumps(
            self.evidence["capabilities"], sort_keys=True, separators=(",", ":")
        ).encode()
        digest = hashlib.sha256(canonical).hexdigest()
        self.assertEqual(
            digest,
            SPIKE.load_pin()["capabilities_contract_sha256"],
        )
        self.assertEqual(
            self.evidence["capabilities"]["endpoints"]["session_chat"],
            {"method": "POST", "path": "/api/sessions/{session_id}/chat"},
        )
        self.assertEqual(
            self.evidence["capabilities"]["runtime"],
            {
                "description": (
                    "The API server creates a server-side Hermes AIAgent; tools "
                    "execute on the API-server host unless a future explicit "
                    "split-runtime mode is enabled."
                ),
                "mode": "server_agent",
                "split_runtime": False,
                "tool_execution": "server",
            },
        )

    def test_reuse_evidence_fails_the_architecture_gate(self) -> None:
        observations = self.evidence["construction_observations"]
        self.assertEqual(observations["agent_factory_calls"], 62)
        self.assertEqual(observations["chat_requests"], 62)
        self.assertEqual(
            observations["distinct_agent_instances_for_repeated_session"], 31
        )
        self.assertFalse(observations["warm_agent_reused"])
        self.assertEqual(
            self.evidence["decision"]["d23_http_warm_agent_gate"], "failed"
        )

    def test_raw_latency_samples_reproduce_summaries(self) -> None:
        trial = self.evidence["trial"]
        self.assertEqual(
            len(trial["new_session_raw_ms"]), trial["samples_per_class"]
        )
        self.assertEqual(
            len(trial["repeated_session_raw_ms"]), trial["samples_per_class"]
        )
        self.assertEqual(
            SPIKE.latency_summary(trial["new_session_raw_ms"]),
            trial["new_session_summary"],
        )
        self.assertEqual(
            SPIKE.latency_summary(trial["repeated_session_raw_ms"]),
            trial["repeated_session_summary"],
        )

    def test_evidence_is_sanitized_and_scoped(self) -> None:
        text = self.evidence_path.read_text(encoding="utf-8")
        for forbidden in (
            "/Users/",
            "/home/",
            "TELEGRAM_BOT_TOKEN",
            "OPENROUTER_API_KEY",
            "XAI_API_KEY",
        ):
            self.assertNotIn(forbidden, text)
        self.assertEqual(
            self.evidence["provider_boundary"], "deterministic_local_fake"
        )
        self.assertIn("Raspberry Pi latency", self.evidence["claims_excluded"])


if __name__ == "__main__":
    unittest.main()
