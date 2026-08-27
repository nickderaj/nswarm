#!/usr/bin/env python3
"""Fail-closed repository policy checks with actionable diagnostics."""

from __future__ import annotations

from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent
SKIP_PARTS = {".git", "target", ".venv"}
SECRET_PATTERNS = {
    "private key": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    "GitHub token": re.compile(r"\bgh[oprsu]_[A-Za-z0-9_]{30,}\b"),
    "provider key": re.compile(r"\bsk-[A-Za-z0-9]{20,}\b"),
}


def repository_paths() -> list[Path]:
    return sorted(
        path
        for path in ROOT.rglob("*")
        if not any(part in SKIP_PARTS for part in path.relative_to(ROOT).parts)
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


def main() -> int:
    errors: list[str] = []
    all_paths = repository_paths()
    for path in all_paths:
        if path.is_symlink():
            errors.append(f"{path.relative_to(ROOT).as_posix()}: repository symlinks are prohibited")
    all_files = [path for path in all_paths if path.is_file() and not path.is_symlink()]

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
                "unsafe Rust": r"\bunsafe(?:\s|/\*.*?\*/|//[^\n]*\n)+(?:fn|impl|trait|extern|\{)",
                "ignored test": r"#\s*\[[^\]]*\bignore\b[^\]]*\]",
                "focused test marker": r"(?:test|describe|it)\.only\s*\(",
                "crate-wide lint allowance": r"#!\s*\[\s*allow\s*\(",
            }
            for label, pattern in checks.items():
                if re.search(pattern, text, re.DOTALL):
                    errors.append(f"{relative}: {label} is prohibited")
            for match in re.finditer(r"#\s*\[\s*(allow|expect)\s*\((.*?)\)\s*\]", text, re.DOTALL):
                if "reason" not in match.group(2):
                    line = text.count("\n", 0, match.start()) + 1
                    errors.append(f"{relative}:{line}: unreasoned lint suppression")
            test_source = "tests" in path.relative_to(ROOT).parts or "#[cfg(test)]" in text
            if test_source and re.search(
                r"TcpStream::connect|TcpListener::bind|tokio::net|reqwest::|ureq::|hyper::|Command::new\(\s*\"curl\"",
                text,
            ):
                errors.append(f"{relative}: ordinary tests may not use the network")

        if path.name == "Cargo.toml":
            document = tomllib.loads(text)
            if relative != "Cargo.toml" and "package" in document:
                lints = document.get("lints", {})
                rust_lints = lints.get("rust", {})
                inherits = lints.get("workspace") is True
                explicit = (
                    rust_lints.get("unsafe_code") == "forbid"
                    and rust_lints.get("warnings") == "deny"
                )
                if not (inherits or explicit):
                    errors.append(f"{relative}: package must inherit or explicitly enforce root Rust lints")
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
                go_tool = re.search(r"\bgo\s+(?:run|install)\s+[^\s@]+@([^\s]+)", line)
                if go_tool and not re.fullmatch(r"[0-9a-f]{40}", go_tool.group(1)):
                    errors.append(
                        f"{relative}:{line_number}: Go tool must use a full commit SHA"
                    )

    if errors:
        for error in errors:
            print(f"policy: {error}", file=sys.stderr)
        return 1
    print(f"policy: {len(all_files)} files checked")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
