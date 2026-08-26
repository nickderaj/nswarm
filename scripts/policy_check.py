#!/usr/bin/env python3
"""Fail-closed repository policy checks with actionable diagnostics."""

from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent
SKIP_PARTS = {".git", "target", ".venv"}
PROTECTED_PREFIXES = (
    ".github/",
    "scripts/",
    "supply-chain/",
    "audits/",
    "generated/",
    "profiles/",
    "deny.toml",
    "Cargo.toml",
    "rust-toolchain.toml",
)
SECRET_PATTERNS = {
    "private key": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    "GitHub token": re.compile(r"\bgh[oprsu]_[A-Za-z0-9_]{30,}\b"),
    "provider key": re.compile(r"\bsk-[A-Za-z0-9]{20,}\b"),
}


def files() -> list[Path]:
    return sorted(
        path
        for path in ROOT.rglob("*")
        if path.is_file() and not any(part in SKIP_PARTS for part in path.relative_to(ROOT).parts)
    )


def cargo_dependency_tables(document: dict) -> list[tuple[str, dict]]:
    tables: list[tuple[str, dict]] = []
    workspace = document.get("workspace", {})
    if isinstance(workspace.get("dependencies"), dict):
        tables.append(("workspace.dependencies", workspace["dependencies"]))
    for name in ("dependencies", "dev-dependencies", "build-dependencies"):
        if isinstance(document.get(name), dict):
            tables.append((name, document[name]))
    for target, target_table in document.get("target", {}).items():
        for name in ("dependencies", "dev-dependencies", "build-dependencies"):
            if isinstance(target_table.get(name), dict):
                tables.append((f"target.{target}.{name}", target_table[name]))
    return tables


def current_branch() -> str:
    result = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def changed_paths() -> list[str]:
    result = subprocess.run(
        ["git", "diff", "--name-only", "origin/main...HEAD"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    return result.stdout.splitlines() if result.returncode == 0 else []


def main() -> int:
    errors: list[str] = []
    all_files = files()

    for path in all_files:
        relative = path.relative_to(ROOT).as_posix()
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        owner_paths = ("/Users/" + "nick", "/home/" + "nick")
        if any(owner_path in text for owner_path in owner_paths):
            errors.append(f"{relative}: owner-specific absolute path")
        for label, pattern in SECRET_PATTERNS.items():
            if pattern.search(text):
                errors.append(f"{relative}: likely {label}")

        if path.suffix == ".rs":
            checks = {
                "unsafe Rust": r"\bunsafe\s+(?:fn|impl|trait|extern|\{)",
                "ignored test": r"#\s*\[\s*ignore(?:\s|\])",
                "focused test marker": r"(?:test|describe|it)\.only\s*\(",
                "crate-wide lint allowance": r"#!\s*\[\s*allow\s*\(",
            }
            for label, pattern in checks.items():
                if re.search(pattern, text):
                    errors.append(f"{relative}: {label} is prohibited")
            for match in re.finditer(r"#\s*\[\s*(allow|expect)\s*\((.*?)\)\s*\]", text, re.DOTALL):
                if "reason" not in match.group(2):
                    line = text.count("\n", 0, match.start()) + 1
                    errors.append(f"{relative}:{line}: unreasoned lint suppression")
            if "#[cfg(test)]" in text and re.search(
                r"TcpStream::connect|reqwest::|ureq::|Command::new\(\s*\"curl\"", text
            ):
                errors.append(f"{relative}: ordinary tests may not use the network")

        if path.name == "Cargo.toml":
            document = tomllib.loads(text)
            for table_name, table in cargo_dependency_tables(document):
                for dependency, value in table.items():
                    if isinstance(value, str):
                        if not value.startswith("="):
                            errors.append(f"{relative}:{table_name}.{dependency}: version must be exact")
                    elif isinstance(value, dict):
                        if "version" in value and not str(value["version"]).startswith("="):
                            errors.append(f"{relative}:{table_name}.{dependency}: version must be exact")
                        if "git" in value:
                            revision = str(value.get("rev", ""))
                            if not re.fullmatch(r"[0-9a-f]{40}", revision):
                                errors.append(f"{relative}:{table_name}.{dependency}: Git dependency needs full rev")
                        if not ({"path", "workspace", "version", "git"} & set(value)):
                            errors.append(f"{relative}:{table_name}.{dependency}: dependency source missing")

        if relative.startswith(".github/workflows/"):
            for line_number, line in enumerate(text.splitlines(), 1):
                match = re.search(r"\buses:\s*([^\s#]+)", line)
                if match and not re.search(r"@[0-9a-f]{40}$", match.group(1)):
                    errors.append(f"{relative}:{line_number}: action must use a full commit SHA")

    branch = current_branch()
    policy_branch = branch == "overnight/bootstrap" or branch.startswith("policy/")
    if os.environ.get("NSWARM_POLICY_CHANGE_ALLOWED") != "1" and not policy_branch:
        protected = [
            path
            for path in changed_paths()
            if any(path == prefix or path.startswith(prefix) for prefix in PROTECTED_PREFIXES)
        ]
        if protected:
            errors.append(
                "protected policy paths changed outside a dedicated policy branch: " + ", ".join(protected)
            )

    if errors:
        for error in errors:
            print(f"policy: {error}", file=sys.stderr)
        return 1
    print(f"policy: {len(all_files)} files checked")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
