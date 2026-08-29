#!/usr/bin/env python3
"""Run semver checks only for packages that exist in the baseline revision."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def command(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=ROOT,
        check=check,
        capture_output=True,
        text=True,
    )


def main() -> int:
    baseline = sys.argv[1] if len(sys.argv) == 2 else "origin/main"
    if len(sys.argv) > 2:
        print(f"usage: {Path(sys.argv[0]).name} [baseline-revision]", file=sys.stderr)
        return 2
    if command("git", "cat-file", "-e", f"{baseline}:Cargo.toml", check=False).returncode:
        print("semver: initial public API baseline will be established by this merge")
        return 0

    metadata = json.loads(
        command(
            "cargo",
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version=1",
        ).stdout
    )
    workspace_members = set(metadata["workspace_members"])
    packages = [
        package for package in metadata["packages"] if package["id"] in workspace_members
    ]
    new_packages = []
    for package in packages:
        manifest = Path(package["manifest_path"]).resolve().relative_to(ROOT).as_posix()
        exists = command(
            "git", "cat-file", "-e", f"{baseline}:{manifest}", check=False
        ).returncode == 0
        if not exists:
            new_packages.append(package["name"])

    if len(new_packages) == len(packages):
        print("semver: no current workspace package exists in the baseline revision")
        return 0
    semver = [
        "cargo",
        "semver-checks",
        "check-release",
        "--workspace",
        "--baseline-rev",
        baseline,
    ]
    for package in sorted(new_packages):
        print(f"semver: {package} is new; establishing its initial public API baseline")
        semver.extend(("--exclude", package))
    subprocess.run(semver, cwd=ROOT, check=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
