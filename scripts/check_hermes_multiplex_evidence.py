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
            "supplemental",
            "decision",
        },
        "root",
    )
    if document["schema_version"] != 2:
        raise EvidenceError("unsupported evidence schema version")
    if document["measurement"] != "pinned_hermes_upstream_multiplex_regression_suite":
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
    for name in (
        "os",
        "architecture",
        "python_implementation",
        "python_version",
        "aiohttp_version",
        "pytest_version",
    ):
        if not isinstance(environment[name], str) or not environment[name]:
            raise EvidenceError(f"environment {name} must be a nonempty string")
    if environment["python_implementation"] != "CPython":
        raise EvidenceError("trial must use CPython")
    if environment["target_pi"] is not False or environment["linux_service_manager"] is not False:
        raise EvidenceError("local trial must not claim target-Pi or Linux-service coverage")

    safeguards = document["safeguards"]
    expected_safeguards = {
        "operator_local_only_acknowledgement": True,
        "exact_source_pin_required": True,
        "tracked_source_clean_required": True,
        "child_environment_allowlist_enforced": True,
        "credential_variables_forwarded": 0,
        "test_file_retries": 0,
        "max_workers": 4,
        "raw_test_output_persisted": False,
    }
    exact_keys(
        safeguards,
        {*expected_safeguards, "forwarded_environment_variable_count"},
        "safeguards",
    )
    if {name: safeguards[name] for name in expected_safeguards} != expected_safeguards:
        raise EvidenceError("local-trial safeguards differ")
    forwarded = _integer(
        safeguards["forwarded_environment_variable_count"],
        "forwarded environment variable count",
    )
    if forwarded > 6:
        raise EvidenceError("child environment exceeds the reviewed allowlist")

    trial = document["trial"]
    if not isinstance(trial, dict):
        raise EvidenceError("trial must be an object")
    exact_keys(
        trial,
        {
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
    _integer(trial["test_files"], "trial test files", positive=True)
    _integer(trial["tests_passed"], "trial tests passed", positive=True)
    if _integer(trial["tests_failed"], "trial tests failed") != 0:
        raise EvidenceError("upstream regression suite failed")
    _integer(trial["tests_skipped_optional_dependencies"], "trial tests skipped")
    if _integer(trial["completion_percent"], "trial completion") != 100:
        raise EvidenceError("upstream regression suite is incomplete")
    workers = _integer(trial["workers"], "trial workers", positive=True)
    if workers > safeguards["max_workers"]:
        raise EvidenceError("trial workers exceed the concurrency cap")
    if _integer(trial["flaky_files"], "trial flaky files") != 0:
        raise EvidenceError("upstream regression suite used a retry")
    runner_ms = _integer(
        trial["canonical_runner_wall_ms"], "canonical runner wall time", positive=True
    )
    harness_ms = _integer(trial["harness_wall_ms"], "harness wall time", positive=True)
    if harness_ms < runner_ms:
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
    contract_keys = {
        "test_files",
        "tests_passed",
        "tests_failed",
        "tests_skipped",
        "result",
    }
    contract_totals = {"files": 0, "passed": 0, "failed": 0, "skipped": 0}
    contract_results: list[str] = []
    for name, contract in contracts.items():
        if not isinstance(contract, dict):
            raise EvidenceError(f"contract {name} must be an object")
        exact_keys(contract, contract_keys, f"contract {name}")
        files = _integer(contract["test_files"], f"contract {name} files", positive=True)
        passed = _integer(contract["tests_passed"], f"contract {name} passed")
        failed = _integer(contract["tests_failed"], f"contract {name} failed")
        skipped = _integer(contract["tests_skipped"], f"contract {name} skipped")
        derived = "failed" if failed else "incomplete" if skipped or not passed else "passed"
        if contract["result"] != derived:
            raise EvidenceError(f"contract {name} result is not derived from its counts")
        contract_totals["files"] += files
        contract_totals["passed"] += passed
        contract_totals["failed"] += failed
        contract_totals["skipped"] += skipped
        contract_results.append(derived)

    supplemental = document["supplemental"]
    if not isinstance(supplemental, dict):
        raise EvidenceError("supplemental result must be an object")
    exact_keys(supplemental, contract_keys, "supplemental")
    supplemental_files = _integer(supplemental["test_files"], "supplemental files")
    supplemental_passed = _integer(supplemental["tests_passed"], "supplemental passed")
    supplemental_failed = _integer(supplemental["tests_failed"], "supplemental failed")
    supplemental_skipped = _integer(supplemental["tests_skipped"], "supplemental skipped")
    supplemental_result = (
        "failed"
        if supplemental_failed
        else "incomplete"
        if supplemental_skipped or not supplemental_passed
        else "passed"
    )
    if supplemental["result"] != supplemental_result:
        raise EvidenceError("supplemental result is not derived from its counts")
    if contract_totals["files"] + supplemental_files != trial["test_files"]:
        raise EvidenceError("test-file totals are not derived from grouped results")
    if contract_totals["passed"] + supplemental_passed != trial["tests_passed"]:
        raise EvidenceError("passing-test totals are not derived from grouped results")
    if contract_totals["failed"] + supplemental_failed != trial["tests_failed"]:
        raise EvidenceError("failed-test totals are not derived from grouped results")
    if contract_totals["skipped"] + supplemental_skipped != trial["tests_skipped_optional_dependencies"]:
        raise EvidenceError("skipped-test totals are not derived from grouped results")

    suite_result = (
        "failed"
        if trial["tests_failed"] or "failed" in contract_results
        else "incomplete"
        if any(result != "passed" for result in contract_results)
        else "passed"
    )
    expected_decision = {
        "upstream_regression_suite": suite_result,
        "nswarm_runtime_isolation_gate": "not_measured",
        "d24_topology": "pending_nswarm_runtime_trial",
        "target_pi_resource_gate": "pending",
        "linux_sandbox_and_socket_acl_gate": "pending",
        "profile_prompt_size_gate": "pending_real_profiles",
        "gym_mcp_attachment": "not_run",
        "live_pilot": False,
    }
    if document["decision"] != expected_decision:
        raise EvidenceError("D24 decision is not derived from the measured scope")

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
