#!/usr/bin/env python3
"""Fail-closed integrity check for committed D24 local simulation evidence."""

from __future__ import annotations

import json
from pathlib import Path
import sys
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_PATH = ROOT / "spikes" / "hermes" / "evidence" / "multiplex-local.json"
PIN_PATH = ROOT / "spikes" / "hermes" / "pin.json"


class EvidenceError(ValueError):
    """Committed D24 evidence is incomplete, unsafe, or inconsistent."""


def exact_keys(document: dict[str, Any], expected: set[str], location: str) -> None:
    if set(document) != expected:
        raise EvidenceError(f"{location} schema differs")


def _integer(value: Any, location: str, *, positive: bool = False) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise EvidenceError(f"{location} must be a nonnegative integer")
    if positive and value == 0:
        raise EvidenceError(f"{location} must be positive")
    return value


def validate_evidence(document: dict[str, Any], pin: dict[str, Any]) -> None:
    exact_keys(
        document,
        {
            "schema_version",
            "measurement",
            "pin",
            "environment",
            "safeguards",
            "trial",
            "contracts",
            "decision",
        },
        "root",
    )
    if document["schema_version"] != 1:
        raise EvidenceError("unsupported evidence schema version")
    if document["measurement"] != "pinned_hermes_multiplex_local_simulation":
        raise EvidenceError("unexpected measurement")

    expected_pin = {
        name: pin[name]
        for name in (
            "repository",
            "tag",
            "tag_object_sha",
            "commit_sha",
            "plan_source_commit_prefix",
            "package",
            "package_version",
            "python_requires",
        )
    }
    if document["pin"] != expected_pin:
        raise EvidenceError("evidence source identity differs from the SHA-256 pin")

    environment = document["environment"]
    if not isinstance(environment, dict):
        raise EvidenceError("environment must be an object")
    exact_keys(
        environment,
        {
            "os",
            "architecture",
            "python_implementation",
            "python_version",
            "aiohttp_version",
            "pytest_version",
            "target_pi",
            "linux_service_manager",
        },
        "environment",
    )
    if environment != {
        "os": "Darwin",
        "architecture": "arm64",
        "python_implementation": "CPython",
        "python_version": "3.11.14",
        "aiohttp_version": "3.14.3",
        "pytest_version": "9.1.1",
        "target_pi": False,
        "linux_service_manager": False,
    }:
        raise EvidenceError("local measurement environment differs")

    safeguards = document["safeguards"]
    expected_safeguards = {
        "operator_local_only_acknowledgement": True,
        "exact_source_pin_required": True,
        "tracked_source_clean_required": True,
        "provider_calls": 0,
        "production_credentials_loaded": 0,
        "test_file_retries": 0,
        "max_workers": 4,
        "raw_test_output_persisted": False,
    }
    if safeguards != expected_safeguards:
        raise EvidenceError("local-trial safeguards differ")

    trial = document["trial"]
    if not isinstance(trial, dict):
        raise EvidenceError("trial must be an object")
    exact_keys(
        trial,
        {
            "profile_count",
            "test_files",
            "tests_passed",
            "tests_failed",
            "tests_skipped_optional_dependencies",
            "completion_percent",
            "workers",
            "canonical_runner_wall_ms",
            "harness_wall_ms",
            "flaky_files",
        },
        "trial",
    )
    expected_counts = {
        "profile_count": 2,
        "test_files": 27,
        "tests_passed": 288,
        "tests_failed": 0,
        "tests_skipped_optional_dependencies": 2,
        "completion_percent": 100,
        "workers": 4,
        "flaky_files": 0,
    }
    for name, expected in expected_counts.items():
        if _integer(trial.get(name), f"trial {name}") != expected:
            raise EvidenceError(f"trial {name} differs")
    runner_ms = _integer(
        trial["canonical_runner_wall_ms"], "canonical runner wall time", positive=True
    )
    harness_ms = _integer(trial["harness_wall_ms"], "harness wall time", positive=True)
    if runner_ms > 180_000 or harness_ms > 180_000 or harness_ms < runner_ms:
        raise EvidenceError("local trial timing is inconsistent")

    contracts = document["contracts"]
    expected_contracts = {
        "profile_route_and_allowlist",
        "per_profile_http_bearer_auth",
        "credential_and_provider_secret_isolation",
        "soul_memory_skill_and_config_scope",
        "session_key_and_sqlite_store_isolation",
        "concurrent_context_and_background_scope",
        "multiplex_adapter_lifecycle",
    }
    if not isinstance(contracts, dict):
        raise EvidenceError("contracts must be an object")
    exact_keys(contracts, expected_contracts, "contracts")
    if any(value != "passed" for value in contracts.values()):
        raise EvidenceError("a local multiplex contract did not pass")

    expected_decision = {
        "local_multiplex_functional_gate": "passed",
        "d24_topology": "provisionally_supports_one_gateway",
        "target_pi_resource_gate": "pending",
        "linux_sandbox_and_socket_acl_gate": "pending",
        "profile_prompt_size_gate": "pending_real_profiles",
        "gym_mcp_attachment": "not_run",
        "live_pilot": False,
    }
    if document["decision"] != expected_decision:
        raise EvidenceError("D24 local-only decision differs")

    serialized = json.dumps(document, sort_keys=True)
    for forbidden in (
        "/Users/",
        "/home/",
        "inf_",
        "sk-ant-",
        "SURPLUS_API_KEY",
        "TELEGRAM_BOT_TOKEN",
        "response_text",
        "request_id",
    ):
        if forbidden in serialized:
            raise EvidenceError("evidence contains raw or secret material")


def main() -> int:
    try:
        document = json.loads(EVIDENCE_PATH.read_text(encoding="utf-8"))
        pin = json.loads(PIN_PATH.read_text(encoding="utf-8"))
        validate_evidence(document, pin)
    except (EvidenceError, OSError, json.JSONDecodeError, KeyError) as exc:
        print(f"Hermes D24 evidence check failed: {exc}", file=sys.stderr)
        return 1
    print("Hermes D24 local simulation evidence: valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
