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
CRITICAL_MARKER = "// coverage-critical"
MINIMUM_CRITICAL_FUNCTIONS = 30


def percentage(covered: int, total: int) -> float:
    return 0.0 if total == 0 else covered * 100.0 / total


def critical_functions() -> list[tuple[Path, int, str]]:
    markers: list[tuple[Path, int, str]] = []
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        lines = path.read_text(encoding="utf-8").splitlines()
        for index, line in enumerate(lines):
            if line.strip() != CRITICAL_MARKER:
                continue
            for function_index in range(index + 1, min(index + 7, len(lines))):
                match = re.match(
                    r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?"
                    r"fn\s+([A-Za-z0-9_]+)",
                    lines[function_index],
                )
                if match is not None:
                    markers.append(
                        (path.relative_to(ROOT), function_index + 1, match.group(1))
                    )
                    break
            else:
                raise ValueError(f"{path.relative_to(ROOT)}:{index + 1}: marker has no function")
    if len(markers) < MINIMUM_CRITICAL_FUNCTIONS:
        raise ValueError(
            f"only {len(markers)} critical functions marked; expected at least "
            f"{MINIMUM_CRITICAL_FUNCTIONS}"
        )
    return markers


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
    changed_value = (
        100.0 if changed_total == 0 else percentage(changed_covered, changed_total)
    )
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
    try:
        markers = critical_functions()
    except ValueError as error:
        print(f"coverage: {error}", file=sys.stderr)
        return 1
    for path, line, name in markers:
        matches = [
            function
            for function in data["functions"]
            if name in function["name"]
            and any(relative(filename) == path for filename in function["filenames"])
            and any(region[0] == line for region in function["regions"])
        ]
        if not matches:
            failures.append(
                f"critical marker {path}:{line} {name} matched no compiled function"
            )
            continue
        branch_shapes = {
            tuple(tuple(branch[:4] + branch[6:]) for branch in function["branches"])
            for function in matches
        }
        if len(branch_shapes) != 1:
            failures.append(
                f"critical marker {path}:{line} {name} had inconsistent branch maps "
                f"across {len(matches)} compiled copies"
            )
            continue
        branches = matches[0]["branches"]
        for branch_index, branch in enumerate(branches):
            true_covered = any(
                function["branches"][branch_index][4] > 0 for function in matches
            )
            false_covered = any(
                function["branches"][branch_index][5] > 0 for function in matches
            )
            critical_total += 2
            critical_covered += int(true_covered) + int(false_covered)
            missing = [
                outcome
                for outcome, covered in (
                    ("true", true_covered),
                    ("false", false_covered),
                )
                if not covered
            ]
            if missing:
                failures.append(
                    f"critical branch {path}:{branch[0]} {name} missing "
                    f"{'/'.join(missing)} outcome"
                )
    critical_value = percentage(critical_covered, critical_total)
    print(
        f"coverage: critical branch outcomes across {len(markers)} marked functions: "
        f"{critical_covered}/{critical_total} "
        f"({critical_value:.2f}%)"
    )
    if critical_total == 0:
        failures.append("critical branch inventory matched no branch outcomes")
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
