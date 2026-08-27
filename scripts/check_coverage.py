#!/usr/bin/env python3
"""Enforce the repository's line, changed-line, and critical-branch contract."""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
REPOSITORY_LINE_MINIMUM = 90.0
PER_CRATE_LINE_MINIMUM = 90.0
CHANGED_LINE_MINIMUM = 95.0
CRITICAL_BRANCH_MINIMUM = 100.0
CRITICAL_FUNCTIONS = (
    "authorize_merge",
    "record_merged",
    "acquire_lease",
    "accept_worker_result",
    "path_resources_overlap",
    "redact_evidence",
    "contains_secret_shape",
    "WorktreeProvisioner9provision",
    "CredentialBroker5issue",
)


def percentage(covered: int, total: int) -> float:
    return 100.0 if total == 0 else covered * 100.0 / total


def relative(filename: str) -> Path | None:
    path = Path(filename).resolve()
    try:
        return path.relative_to(ROOT)
    except ValueError:
        return None


def changed_lines(base: str) -> dict[Path, set[int]]:
    result = subprocess.run(
        [
            "git",
            "diff",
            "--unified=0",
            f"{base}...HEAD",
            "--",
            "*.rs",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    current: Path | None = None
    lines: dict[Path, set[int]] = {}
    for line in result.stdout.splitlines():
        if line.startswith("+++ b/"):
            current = Path(line.removeprefix("+++ b/"))
            lines.setdefault(current, set())
            continue
        if current is None or not line.startswith("@@"):
            continue
        match = re.search(r"\+(\d+)(?:,(\d+))?", line)
        if match is None:
            continue
        start = int(match.group(1))
        count = int(match.group(2) or "1")
        lines[current].update(range(start, start + count))
    return lines


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: check_coverage.py <llvm-cov.json> <base-revision>", file=sys.stderr)
        return 2

    document = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    data = document["data"][0]
    failures: list[str] = []
    totals = data["totals"]["lines"]
    repository_lines = float(totals["percent"])
    if repository_lines < REPOSITORY_LINE_MINIMUM:
        failures.append(
            f"repository line coverage {repository_lines:.2f}% < {REPOSITORY_LINE_MINIMUM:.2f}%"
        )

    crate_lines: dict[str, list[int]] = {}
    executable_lines: dict[Path, dict[int, bool]] = {}
    for file in data["files"]:
        path = relative(file["filename"])
        if path is None:
            continue
        parts = path.parts
        if len(parts) >= 3 and parts[0] == "crates":
            covered = int(file["summary"]["lines"]["covered"])
            count = int(file["summary"]["lines"]["count"])
            aggregate = crate_lines.setdefault(parts[1], [0, 0])
            aggregate[0] += covered
            aggregate[1] += count
        line_map = executable_lines.setdefault(path, {})
        for segment in file["segments"]:
            line_number, _, count, has_count, _, is_gap = segment
            if has_count and not is_gap:
                line_map[line_number] = line_map.get(line_number, False) or count > 0

    for crate, (covered, count) in sorted(crate_lines.items()):
        value = percentage(covered, count)
        print(f"coverage: crate {crate}: {covered}/{count} lines ({value:.2f}%)")
        if value < PER_CRATE_LINE_MINIMUM:
            failures.append(
                f"crate {crate} line coverage {value:.2f}% < {PER_CRATE_LINE_MINIMUM:.2f}%"
            )

    changed = changed_lines(sys.argv[2])
    changed_total = 0
    changed_covered = 0
    for path, line_numbers in changed.items():
        for line_number in line_numbers:
            if line_number in executable_lines.get(path, {}):
                changed_total += 1
                changed_covered += int(executable_lines[path][line_number])
    changed_value = percentage(changed_covered, changed_total)
    print(
        f"coverage: changed executable lines: {changed_covered}/{changed_total} "
        f"({changed_value:.2f}%)"
    )
    if changed_value < CHANGED_LINE_MINIMUM:
        failures.append(
            f"changed-line coverage {changed_value:.2f}% < {CHANGED_LINE_MINIMUM:.2f}%"
        )

    critical_total = 0
    critical_covered = 0
    for function in data["functions"]:
        if not function["name"].startswith("_RNv") or not any(
            name in function["name"] for name in CRITICAL_FUNCTIONS
        ):
            continue
        for branch in function["branches"]:
            critical_total += 2
            critical_covered += int(branch[4] > 0) + int(branch[5] > 0)
    critical_value = percentage(critical_covered, critical_total)
    print(
        f"coverage: critical branch outcomes: {critical_covered}/{critical_total} "
        f"({critical_value:.2f}%)"
    )
    if critical_value < CRITICAL_BRANCH_MINIMUM:
        failures.append(
            f"critical branch coverage {critical_value:.2f}% < {CRITICAL_BRANCH_MINIMUM:.2f}%"
        )

    print(
        f"coverage: repository lines: {int(totals['covered'])}/{int(totals['count'])} "
        f"({repository_lines:.2f}%)"
    )
    if failures:
        for failure in failures:
            print(f"coverage: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
