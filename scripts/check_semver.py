#!/usr/bin/env python3
"""Run semver checks for every package identity present in both revisions."""

from __future__ import annotations

import json
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def command(
    *args: str, check: bool = True, cwd: Path = ROOT
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        check=check,
        capture_output=True,
        text=True,
    )


def workspace_package_names(metadata: dict) -> set[str]:
    """Return unique workspace package names from Cargo metadata."""
    workspace_members = set(metadata["workspace_members"])
    names = [
        package["name"]
        for package in metadata["packages"]
        if package["id"] in workspace_members
    ]
    if len(names) != len(set(names)):
        raise ValueError("workspace package names must be unique")
    return set(names)


def package_delta(
    current_names: set[str], baseline_names: set[str]
) -> tuple[list[str], list[str]]:
    """Return new and removed package identities in deterministic order."""
    return sorted(current_names - baseline_names), sorted(baseline_names - current_names)


def current_workspace_package_names() -> set[str]:
    """Load the current locked workspace package identities."""
    metadata = json.loads(
        command(
            "cargo",
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version=1",
        ).stdout
    )
    return workspace_package_names(metadata)


def baseline_workspace_package_names(baseline: str) -> set[str]:
    """Load package identities from an isolated archive of the baseline."""
    with tempfile.TemporaryDirectory(prefix="nswarm-semver-baseline-") as temporary:
        temporary_path = Path(temporary)
        archive_path = temporary_path / "baseline.tar"
        tree_path = temporary_path / "tree"
        tree_path.mkdir()
        command(
            "git",
            "archive",
            "--format=tar",
            f"--output={archive_path}",
            baseline,
        )
        with tarfile.open(archive_path) as archive:
            archive.extractall(tree_path, filter="data")
        metadata = json.loads(
            command(
                "cargo",
                "metadata",
                "--locked",
                "--no-deps",
                "--format-version=1",
                "--manifest-path",
                str(tree_path / "Cargo.toml"),
                cwd=tree_path,
            ).stdout
        )
        return workspace_package_names(metadata)


def main() -> int:
    baseline = sys.argv[1] if len(sys.argv) == 2 else "origin/main"
    if len(sys.argv) > 2:
        print(f"usage: {Path(sys.argv[0]).name} [baseline-revision]", file=sys.stderr)
        return 2
    if command("git", "cat-file", "-e", f"{baseline}:Cargo.toml", check=False).returncode:
        print("semver: initial public API baseline will be established by this merge")
        return 0

    current_names = current_workspace_package_names()
    baseline_names = baseline_workspace_package_names(baseline)
    new_packages, removed_packages = package_delta(current_names, baseline_names)
    if removed_packages:
        for package in removed_packages:
            print(
                f"semver: baseline package {package} is missing from the current workspace",
                file=sys.stderr,
            )
        print(
            "semver: refusing to treat package removal or rename as a new crate",
            file=sys.stderr,
        )
        return 1

    if len(new_packages) == len(current_names):
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
    for package in new_packages:
        print(f"semver: {package} is new; establishing its initial public API baseline")
        semver.extend(("--exclude", package))
    subprocess.run(semver, cwd=ROOT, check=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
