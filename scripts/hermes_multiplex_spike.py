#!/usr/bin/env python3
"""Run the pinned, local-only D24 Hermes multiplexing simulation."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_DIR = (ROOT / "spikes" / "hermes" / "evidence").resolve()
DEFAULT_OUTPUT = EVIDENCE_DIR / "multiplex-local.json"
MAX_WORKERS = 4

_GATEWAY_SPEC = importlib.util.spec_from_file_location(
    "hermes_gateway_spike_for_multiplex",
    ROOT / "scripts" / "hermes_gateway_spike.py",
)
assert _GATEWAY_SPEC is not None and _GATEWAY_SPEC.loader is not None
GATEWAY = importlib.util.module_from_spec(_GATEWAY_SPEC)
_GATEWAY_SPEC.loader.exec_module(GATEWAY)


TEST_FILES = (
    "tests/agent/test_secret_scope.py",
    "tests/agent/test_secret_scope_tier1_migration.py",
    "tests/agent/test_soul_md_profile_isolation.py",
    "tests/cron/test_cron_profile_isolation.py",
    "tests/gateway/test_64674_multiplex_primary_token_scope.py",
    "tests/gateway/test_75349_whatsapp_multiplex_secret_scope.py",
    "tests/gateway/test_adapter_startup_secret_scope.py",
    "tests/gateway/test_api_server_multiplex_secret_scope.py",
    "tests/gateway/test_email_secret_scope.py",
    "tests/gateway/test_multiplex_adapter_registry.py",
    "tests/gateway/test_multiplex_adapter_session_key_namespace.py",
    "tests/gateway/test_multiplex_api_server_routing.py",
    "tests/gateway/test_multiplex_background_task_scope.py",
    "tests/gateway/test_multiplex_busy_input_mode.py",
    "tests/gateway/test_multiplex_credential_isolation.py",
    "tests/gateway/test_multiplex_http_routing.py",
    "tests/gateway/test_multiplex_lifecycle.py",
    "tests/gateway/test_multiplex_pairing_stores.py",
    "tests/gateway/test_multiplex_phase0.py",
    "tests/gateway/test_multiplex_profile_authz.py",
    "tests/gateway/test_multiplex_session_db_profile_scope.py",
    "tests/gateway/test_profile_routing.py",
    "tests/gateway/test_weixin_secret_scope.py",
    "tests/hermes_cli/test_gateway_enroll_multiplex_warning.py",
    "tests/hermes_cli/test_model_picker_secret_scope.py",
    "tests/test_profile_isolation_runtime.py",
    "tests/test_secret_scope_plugin_families.py",
)

CONTRACT_TEST_FILES = {
    "profile_route_and_allowlist": (
        "tests/gateway/test_multiplex_http_routing.py",
        "tests/gateway/test_multiplex_api_server_routing.py",
        "tests/gateway/test_profile_routing.py",
    ),
    "per_profile_http_bearer_auth": (
        "tests/gateway/test_64674_multiplex_primary_token_scope.py",
        "tests/gateway/test_api_server_multiplex_secret_scope.py",
        "tests/gateway/test_multiplex_profile_authz.py",
    ),
    "credential_and_provider_secret_isolation": (
        "tests/agent/test_secret_scope.py",
        "tests/gateway/test_adapter_startup_secret_scope.py",
        "tests/gateway/test_email_secret_scope.py",
        "tests/gateway/test_multiplex_credential_isolation.py",
        "tests/gateway/test_weixin_secret_scope.py",
        "tests/hermes_cli/test_model_picker_secret_scope.py",
        "tests/test_secret_scope_plugin_families.py",
    ),
    "soul_memory_skill_and_config_scope": (
        "tests/agent/test_soul_md_profile_isolation.py",
        "tests/cron/test_cron_profile_isolation.py",
        "tests/test_profile_isolation_runtime.py",
    ),
    "session_key_and_sqlite_store_isolation": (
        "tests/gateway/test_multiplex_adapter_session_key_namespace.py",
        "tests/gateway/test_multiplex_pairing_stores.py",
        "tests/gateway/test_multiplex_session_db_profile_scope.py",
    ),
    "concurrent_context_and_background_scope": (
        "tests/gateway/test_75349_whatsapp_multiplex_secret_scope.py",
        "tests/gateway/test_multiplex_background_task_scope.py",
        "tests/gateway/test_multiplex_busy_input_mode.py",
    ),
    "multiplex_adapter_lifecycle": (
        "tests/gateway/test_multiplex_adapter_registry.py",
        "tests/gateway/test_multiplex_lifecycle.py",
        "tests/gateway/test_multiplex_phase0.py",
        "tests/hermes_cli/test_gateway_enroll_multiplex_warning.py",
    ),
}
SUPPLEMENTAL_TEST_FILES = ("tests/agent/test_secret_scope_tier1_migration.py",)
CHILD_ENV_ALLOWLIST = ("HOME", "LANG", "LC_ALL", "PATH", "TMPDIR", "TZ")

SUMMARY_RE = re.compile(
    r"=== Summary: (?P<files>\d+) files, (?P<passed>\d+) tests passed, "
    r"(?P<failed>\d+) failed, (?P<skipped>\d+) skipped "
    r"\((?P<complete>\d+)% complete\) in (?P<seconds>\d+(?:\.\d+)?)s "
    r"\((?P<workers>\d+) workers\) ==="
)
FILE_RESULT_RE = re.compile(
    r"^\[[^\n]+\]\s+[✓✗]\s+(?P<file>tests/\S+)\s+"
    r"\((?P<summary>[^)]*)\)$",
    re.MULTILINE,
)
FILE_COUNT_RE = re.compile(
    r"(?:^|\s)(?P<count>\d+)(?P<kind>xf|xp|✓|✗|s|e)(?=\s|,|$)"
)


class MultiplexSpikeError(ValueError):
    """The local D24 trial was unsafe, incomplete, or inconsistent."""


def _pin_identity(pin: dict[str, Any]) -> dict[str, Any]:
    return {
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


def validate_args(args: argparse.Namespace) -> tuple[Path, Path, Path, int]:
    if not args.i_understand_this_is_local_only:
        raise MultiplexSpikeError("refusing without --i-understand-this-is-local-only")
    source = args.source.resolve()
    # Preserve a virtualenv's interpreter symlink. Resolving it to the base
    # CPython binary changes sys.prefix and silently drops the environment.
    python = Path(os.path.abspath(args.python))
    output = args.output.resolve()
    if not source.is_dir():
        raise MultiplexSpikeError("Hermes source is not a directory")
    if not python.is_file() or not os.access(python, os.X_OK):
        raise MultiplexSpikeError("--python must be an executable pinned-environment Python")
    if output.parent != EVIDENCE_DIR:
        raise MultiplexSpikeError("output must be directly under spikes/hermes/evidence")
    if not 1 <= args.workers <= MAX_WORKERS:
        raise MultiplexSpikeError(f"workers must be between 1 and {MAX_WORKERS}")
    return source, python, output, args.workers


def parse_file_results(output: str) -> dict[str, dict[str, int]]:
    results: dict[str, dict[str, int]] = {}
    for match in FILE_RESULT_RE.finditer(output):
        filename = match.group("file")
        if filename not in TEST_FILES:
            continue
        if filename in results:
            raise MultiplexSpikeError("canonical Hermes output repeated a test file")
        counts = {
            "passed": 0,
            "failed": 0,
            "skipped": 0,
        }
        for count_match in FILE_COUNT_RE.finditer(match.group("summary")):
            count = int(count_match.group("count"))
            kind = count_match.group("kind")
            if kind == "✓":
                counts["passed"] += count
            elif kind in {"✗", "e", "xp"}:
                counts["failed"] += count
            elif kind in {"s", "xf"}:
                counts["skipped"] += count
        if sum(counts.values()) == 0:
            raise MultiplexSpikeError("canonical Hermes file result omitted test counts")
        results[filename] = counts
    if set(results) != set(TEST_FILES):
        raise MultiplexSpikeError("canonical Hermes per-file results are incomplete")
    return results


def summarize_group(
    filenames: tuple[str, ...], per_file: dict[str, dict[str, int]]
) -> dict[str, int | str]:
    passed = sum(per_file[name]["passed"] for name in filenames)
    failed = sum(per_file[name]["failed"] for name in filenames)
    skipped = sum(per_file[name]["skipped"] for name in filenames)
    if failed:
        result = "failed"
    elif skipped or not passed:
        result = "incomplete"
    else:
        result = "passed"
    return {
        "test_files": len(filenames),
        "tests_passed": passed,
        "tests_failed": failed,
        "tests_skipped": skipped,
        "result": result,
    }


def parse_summary(output: str, expected_workers: int) -> tuple[dict[str, int], dict[str, dict[str, int]]]:
    matches = list(SUMMARY_RE.finditer(output))
    if len(matches) != 1:
        raise MultiplexSpikeError("canonical Hermes test summary missing or ambiguous")
    groups = matches[0].groupdict()
    values = {
        name: int(value)
        for name, value in groups.items()
        if name != "seconds"
    }
    values["wall_ms"] = round(float(groups["seconds"]) * 1000)
    if values["files"] != len(TEST_FILES):
        raise MultiplexSpikeError("focused test-file count differs")
    if values["passed"] <= 0 or values["failed"] != 0 or values["complete"] != 100:
        raise MultiplexSpikeError("focused Hermes multiplex suite did not pass")
    if values["workers"] != expected_workers:
        raise MultiplexSpikeError("Hermes runner worker count differs")
    if "FLAKY file" in output:
        raise MultiplexSpikeError("focused Hermes multiplex suite required a retry")
    per_file = parse_file_results(output)
    if values["passed"] != sum(item["passed"] for item in per_file.values()):
        raise MultiplexSpikeError("canonical Hermes pass totals are inconsistent")
    if values["failed"] != sum(item["failed"] for item in per_file.values()):
        raise MultiplexSpikeError("canonical Hermes failure totals are inconsistent")
    if values["skipped"] != sum(item["skipped"] for item in per_file.values()):
        parsed_skips = sum(item["skipped"] for item in per_file.values())
        raise MultiplexSpikeError(
            "canonical Hermes skip totals are inconsistent "
            f"({values['skipped']} summary, {parsed_skips} per-file)"
        )
    return values, per_file


def _python_identity(python: Path) -> dict[str, str]:
    probe = subprocess.run(
        [
            str(python),
            "-c",
            (
                "import aiohttp,json,platform,pytest,sys;"
                "print(json.dumps({'implementation':platform.python_implementation(),"
                "'version':platform.python_version(),"
                "'aiohttp':aiohttp.__version__,'pytest':pytest.__version__,"
                "'executable_prefix':sys.prefix!=sys.base_prefix}))"
            ),
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=15,
    )
    if probe.returncode != 0:
        raise MultiplexSpikeError("pinned Python lacks the required test dependencies")
    try:
        identity = json.loads(probe.stdout)
    except json.JSONDecodeError as exc:
        raise MultiplexSpikeError("Python identity probe returned invalid JSON") from exc
    if identity.get("implementation") != "CPython" or not identity.get("executable_prefix"):
        raise MultiplexSpikeError("--python must be CPython from an isolated environment")
    return identity


def run_local_trial(source: Path, python: Path, workers: int) -> dict[str, Any]:
    pin = GATEWAY.load_pin()
    GATEWAY.verify_source(source, pin)
    if GATEWAY.git_output(source, "status", "--porcelain", "--untracked-files=no"):
        raise MultiplexSpikeError("pinned Hermes tracked worktree is dirty")
    missing = [name for name in TEST_FILES if not (source / name).is_file()]
    if missing:
        raise MultiplexSpikeError("focused Hermes test input is missing")
    identity = _python_identity(python)

    command = [
        str(source / "scripts" / "run_tests.sh"),
        "-j",
        str(workers),
        "--file-retries",
        "0",
        *TEST_FILES,
    ]
    environment = {
        name: os.environ[name]
        for name in CHILD_ENV_ALLOWLIST
        if name in os.environ
    }
    environment["HERMES_PYTHON"] = str(python)
    started = time.perf_counter_ns()
    result = subprocess.run(
        command,
        cwd=source,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
        timeout=180,
    )
    elapsed_ms = round((time.perf_counter_ns() - started) / 1_000_000)
    combined = result.stdout + "\n" + result.stderr
    if result.returncode != 0:
        raise MultiplexSpikeError("focused Hermes multiplex suite failed")
    summary, per_file = parse_summary(combined, workers)
    contracts = {
        name: summarize_group(files, per_file)
        for name, files in CONTRACT_TEST_FILES.items()
    }
    supplemental = summarize_group(SUPPLEMENTAL_TEST_FILES, per_file)
    suite_result = (
        "failed"
        if summary["failed"] or any(item["result"] == "failed" for item in contracts.values())
        else "incomplete"
        if any(item["result"] != "passed" for item in contracts.values())
        else "passed"
    )

    return {
        "schema_version": 2,
        "measurement": "pinned_hermes_upstream_multiplex_regression_suite",
        "pin": _pin_identity(pin),
        "environment": {
            "os": platform.system(),
            "architecture": platform.machine(),
            "python_implementation": identity["implementation"],
            "python_version": identity["version"],
            "aiohttp_version": identity["aiohttp"],
            "pytest_version": identity["pytest"],
            "target_pi": False,
            "linux_service_manager": False,
        },
        "safeguards": {
            "operator_local_only_acknowledgement": True,
            "exact_source_pin_required": True,
            "tracked_source_clean_required": True,
            "child_environment_allowlist_enforced": True,
            "forwarded_environment_variable_count": len(environment) - 1,
            "credential_variables_forwarded": 0,
            "test_file_retries": 0,
            "max_workers": MAX_WORKERS,
            "raw_test_output_persisted": False,
        },
        "trial": {
            "test_files": summary["files"],
            "tests_passed": summary["passed"],
            "tests_failed": summary["failed"],
            "tests_skipped_optional_dependencies": summary["skipped"],
            "completion_percent": summary["complete"],
            "workers": summary["workers"],
            "canonical_runner_wall_ms": summary["wall_ms"],
            "harness_wall_ms": elapsed_ms,
            "flaky_files": 0,
        },
        "contracts": contracts,
        "supplemental": supplemental,
        "decision": {
            "upstream_regression_suite": suite_result,
            "nswarm_runtime_isolation_gate": "not_measured",
            "d24_topology": "pending_nswarm_runtime_trial",
            "target_pi_resource_gate": "pending",
            "linux_sandbox_and_socket_acl_gate": "pending",
            "profile_prompt_size_gate": "pending_real_profiles",
            "gym_mcp_attachment": "not_run",
            "live_pilot": False,
        },
    }


def write_evidence(output: Path, document: dict[str, Any]) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + ".tmp")
    temporary.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(output)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--python", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--i-understand-this-is-local-only", action="store_true")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        source, python, output, workers = validate_args(args)
        evidence = run_local_trial(source, python, workers)
        write_evidence(output, evidence)
    except (MultiplexSpikeError, GATEWAY.SpikeError, subprocess.TimeoutExpired) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
