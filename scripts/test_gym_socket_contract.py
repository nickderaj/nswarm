#!/usr/bin/env python3
"""Static cross-platform contract tests for Linux socket verification."""

from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class GymSocketContractTests(unittest.TestCase):
    def test_manifest_runtime_and_verifier_agree(self) -> None:
        manifest = (ROOT / "bots/gym.toml").read_text(encoding="utf-8")
        unit = (ROOT / "generated/systemd/gym.service").read_text(encoding="utf-8")
        tmpfiles = (ROOT / "generated/tmpfiles/nswarm-gym.conf").read_text(
            encoding="utf-8"
        )
        verifier = (ROOT / "scripts/verify_gym_socket_linux.sh").read_text(encoding="utf-8")
        runtime = (ROOT / "crates/gym-bot/src/mcp.rs").read_text(encoding="utf-8")
        self.assertIn('socket = "/run/gym/mcp.sock"', manifest)
        self.assertIn('socket_group = "gym-access"', manifest)
        self.assertIn('peers = ["boss-agent"]', manifest)
        self.assertIn("SupplementaryGroups=gym-access", unit)
        self.assertIn("Environment=NSWARM_MCP_SOCKET=/run/gym/mcp.sock", unit)
        self.assertIn("Environment=NSWARM_MCP_SOCKET_GROUP=gym-access", unit)
        self.assertIn("UMask=0077", unit)
        self.assertNotIn("RuntimeDirectory=", unit)
        self.assertEqual(tmpfiles, "d /run/gym 2750 gym-agent gym-access - -\n")
        self.assertIn("from_mode(0o660)", runtime)
        self.assertIn(
            "hermes-gateway research-agent tutor-agent trading-agent", verifier
        )
        self.assertNotIn("hermes-gateway", manifest)


if __name__ == "__main__":
    unittest.main()
