#!/usr/bin/env python3
"""Run the committed evaluation corpus against named production Rust tests."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "eval" / "corpus"
CASE_FIELDS = {"schema_version", "id", "package", "test", "input", "expected"}
REQUIRED_CASES = {
    "capability-boundaries",
    "coder-containment",
    "exact-sha",
    "path-containment",
    "redaction",
    "research-evidence",
    "serial-pilot",
    "transition-policy",
}
IDENTIFIER = re.compile(r"[a-z][a-z0-9-]*\Z")
TEST_NAME = re.compile(r"[a-z_][a-z0-9_]*(?:::[a-z_][a-z0-9_]*)+\Z")


class EvalError(Exception):
    """A readable corpus or execution failure."""


def load_cases() -> list[dict]:
    paths = sorted(CORPUS.glob("*.json"))
    if not paths:
        raise EvalError("no JSON cases found")

    cases: list[dict] = []
    ids: set[str] = set()
    tests: set[tuple[str, str]] = set()
    for path in paths:
        try:
            case = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise EvalError(f"{path.relative_to(ROOT)}: {error}") from error
        if not isinstance(case, dict) or set(case) != CASE_FIELDS:
            raise EvalError(
                f"{path.relative_to(ROOT)}: expected exactly {sorted(CASE_FIELDS)}"
            )
        case_id = case["id"]
        package = case["package"]
        test = case["test"]
        if type(case["schema_version"]) is not int or case["schema_version"] != 1:
            raise EvalError(f"{path.relative_to(ROOT)}: unsupported schema_version")
        if not isinstance(case_id, str) or not IDENTIFIER.fullmatch(case_id):
            raise EvalError(f"{path.relative_to(ROOT)}: invalid id")
        if path.stem != case_id:
            raise EvalError(f"{path.relative_to(ROOT)}: id must match the file name")
        if not isinstance(package, str) or not IDENTIFIER.fullmatch(package):
            raise EvalError(f"{path.relative_to(ROOT)}: invalid package")
        if (
            not isinstance(test, str)
            or not TEST_NAME.fullmatch(test)
            or "::eval_" not in test
        ):
            raise EvalError(f"{path.relative_to(ROOT)}: invalid named eval test")
        if not isinstance(case["input"], dict) or not isinstance(case["expected"], dict):
            raise EvalError(f"{path.relative_to(ROOT)}: input and expected must be objects")
        if case_id in ids:
            raise EvalError(f"duplicate case id: {case_id}")
        if (package, test) in tests:
            raise EvalError(f"duplicate named test: {package} {test}")
        ids.add(case_id)
        tests.add((package, test))
        cases.append(case)
    if ids != REQUIRED_CASES:
        raise EvalError(
            f"required case set mismatch: expected {sorted(REQUIRED_CASES)}, got {sorted(ids)}"
        )
    return cases


def run_package(package: str, expected_tests: list[str]) -> None:
    command = [
        "cargo",
        "test",
        "--locked",
        "--offline",
        "--color",
        "never",
        "-p",
        package,
        "--lib",
        "--",
        "--nocapture",
    ]
    environment = os.environ.copy()
    environment["CARGO_NET_OFFLINE"] = "true"
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
            timeout=300,
        )
    except subprocess.TimeoutExpired as error:
        raise EvalError(f"{package}: Rust tests exceeded 300 seconds") from error
    output = completed.stdout + completed.stderr
    if completed.returncode != 0:
        sys.stderr.write(output)
        raise EvalError(f"{package}: Rust tests failed with exit {completed.returncode}")

    for test in expected_tests:
        success = re.compile(rf"^test {re.escape(test)} \.\.\. ok$", re.MULTILINE)
        if not success.search(output):
            sys.stderr.write(output)
            raise EvalError(f"{package}: named test did not report success: {test}")


def main() -> int:
    try:
        cases = load_cases()
        packages: dict[str, list[str]] = {}
        for case in cases:
            packages.setdefault(case["package"], []).append(case["test"])
        for package, tests in sorted(packages.items()):
            run_package(package, tests)
    except EvalError as error:
        print(f"eval corpus: {error}", file=sys.stderr)
        return 1
    print(
        f"eval corpus: {len(cases)} production-backed named Rust tests passed "
        f"across {len(packages)} package(s)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
