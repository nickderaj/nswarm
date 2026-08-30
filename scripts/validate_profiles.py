#!/usr/bin/env python3
"""Validate root-owned profile sources and their canonical generated forms."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent
EXPECTED = {
    "gym": {
        "skills": {
            "training-coach",
        },
    },
    "research": {
        "skills": {
            "research-router",
            "how",
            "why",
            "blast-radius",
            "source-critique",
            "technical-writing",
            "show-your-evidence",
        },
    },
    "coder": {
        "skills": {
            "coding-router",
            "how",
            "architect",
            "tdd-or-repro",
            "blast-radius",
            "verify-real-artifact",
            "interrogate",
            "scope-and-diff-review",
            "technical-writing",
            "show-me-your-work",
            "pause-and-handoff",
        },
    },
}


def repository_path(value: str, *, kind: str) -> Path:
    relative = Path(value)
    if relative.is_absolute() or ".." in relative.parts or "." in relative.parts:
        fail(f"{kind}: unsafe repository-relative path {value!r}")
    path = ROOT / relative
    if path.is_symlink():
        fail(f"{kind}: symlinks are prohibited")
    return path


def fail(message: str) -> None:
    raise ValueError(message)


def load_role_capabilities() -> tuple[dict[str, set[str]], set[str]]:
    path = ROOT / "profiles" / "role-capabilities.json"
    document = json.loads(path.read_text(encoding="utf-8"))
    if set(document) != {"schema_version", "roles"} or document["schema_version"] != 1:
        fail(f"{path.relative_to(ROOT)}: unsupported authority-map schema")
    if not isinstance(document["roles"], dict) or not document["roles"]:
        fail(f"{path.relative_to(ROOT)}: role map must not be empty")
    roles: dict[str, set[str]] = {}
    for role, capabilities in document["roles"].items():
        if (
            not isinstance(role, str)
            or not isinstance(capabilities, list)
            or not capabilities
            or not all(isinstance(capability, str) and capability for capability in capabilities)
            or len(capabilities) != len(set(capabilities))
        ):
            fail(f"{path.relative_to(ROOT)}: invalid capability map for {role!r}")
        roles[role] = set(capabilities)
    return roles, set().union(*roles.values())


def parse_frontmatter(path: Path) -> dict[str, str]:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    if len(lines) < 6 or lines[0] != "---" or "---" not in lines[1:]:
        fail(f"{path.relative_to(ROOT)}: malformed frontmatter")
    end = lines[1:].index("---") + 1
    values: dict[str, str] = {}
    for line in lines[1:end]:
        key, separator, value = line.partition(":")
        if not separator or not value.strip():
            fail(f"{path.relative_to(ROOT)}: invalid frontmatter line")
        values[key.strip()] = value.strip()
    if set(values) != {"name", "description", "policy-version"}:
        fail(f"{path.relative_to(ROOT)}: unexpected frontmatter schema")
    if values["policy-version"] != "v1":
        fail(f"{path.relative_to(ROOT)}: policy version drift")
    if len("\n".join(lines[end + 1 :]).strip()) < 80:
        fail(f"{path.relative_to(ROOT)}: skill body is not substantive")
    return values


def validate_memory(path: Path) -> None:
    content = path.read_text(encoding="utf-8")
    if not content.endswith("\n"):
        fail(f"{path.relative_to(ROOT)}: memory must end with one newline")
    body = content[:-1]
    entries = body.split("\n§\n") if body else []
    rendered = "\n§\n".join(entries) + ("\n" if entries else "")
    if rendered != content:
        fail(f"{path.relative_to(ROOT)}: memory delimiter does not round-trip")


def validate_profile(
    name: str,
    expected: dict[str, set[str]],
    role_capabilities: dict[str, set[str]],
    capability_vocabulary: set[str],
) -> None:
    profile_path = ROOT / "profiles" / name / "profile.toml"
    profile = tomllib.loads(profile_path.read_text(encoding="utf-8"))
    required = {
        "profile_version",
        "policy_version",
        "role",
        "soul",
        "skills",
        "memory",
        "capabilities",
        "forbidden_capabilities",
        "governance",
        "network",
    }
    if set(profile) != required:
        fail(f"{name}: profile schema fields differ")
    if profile["profile_version"] != 1 or profile["policy_version"] != "v1":
        fail(f"{name}: unsupported profile or policy version")
    if profile["role"] != name:
        fail(f"{name}: role mismatch")
    granted = role_capabilities.get(name)
    if granted is None:
        fail(f"{name}: role missing from authority map")
    if set(profile["capabilities"]) != granted:
        fail(f"{name}: capabilities differ from Rust-checked authority map")
    if set(profile["forbidden_capabilities"]) != capability_vocabulary - granted:
        fail(f"{name}: forbidden capabilities are not the exhaustive authority complement")
    if profile["governance"] != {
        "write_approval": True,
        "background_review": False,
        "curator": False,
    }:
        fail(f"{name}: governance must fail closed")
    if profile["network"] != {"default": "deny", "brief_allow_list_required": True}:
        fail(f"{name}: network policy must default deny")

    soul_path = repository_path(profile["soul"], kind=f"{name} soul")
    soul = soul_path.read_text(encoding="utf-8").lower()
    for phrase in ("immutable", "untrusted attributed", "cannot", "evidence"):
        if phrase not in soul:
            fail(f"{name}: SOUL missing required mechanism {phrase!r}")

    skill_root = repository_path(profile["skills"], kind=f"{name} skills")
    actual_skills = {path.parent.name for path in skill_root.glob("*/SKILL.md")}
    if actual_skills != expected["skills"]:
        fail(f"{name}: skill inventory drift")
    for skill in sorted(actual_skills):
        metadata = parse_frontmatter(skill_root / skill / "SKILL.md")
        if metadata["name"] != skill:
            fail(f"{name}/{skill}: name must match directory")

    memory_root = repository_path(profile["memory"], kind=f"{name} memory")
    validate_memory(memory_root / "MEMORY.md")

    canonical = json.dumps(profile, indent=2, sort_keys=True) + "\n"
    generated_path = ROOT / "generated" / "profiles" / f"{name}.json"
    if not generated_path.exists() or generated_path.read_text(encoding="utf-8") != canonical:
        fail(f"{generated_path.relative_to(ROOT)}: generated profile drift")
    if json.loads(canonical) != profile:
        fail(f"{name}: canonical profile does not round-trip")


def main() -> int:
    try:
        role_capabilities, capability_vocabulary = load_role_capabilities()
        for root_name in ("profiles", "generated/profiles"):
            root = ROOT / root_name
            symlinks = [path for path in root.rglob("*") if path.is_symlink()]
            if symlinks:
                fail(f"{symlinks[0].relative_to(ROOT)}: symlinks are prohibited")
        expected_generated = {f"{name}.json" for name in EXPECTED}
        actual_generated = {
            path.name for path in (ROOT / "generated" / "profiles").iterdir() if path.is_file()
        }
        if actual_generated != expected_generated:
            fail("generated profile inventory drift")
        for profile_name, expected in EXPECTED.items():
            validate_profile(
                profile_name,
                expected,
                role_capabilities,
                capability_vocabulary,
            )
    except (OSError, ValueError, tomllib.TOMLDecodeError, json.JSONDecodeError) as error:
        print(f"profile policy: {error}", file=sys.stderr)
        return 1
    print(f"profile policy: {len(EXPECTED)} profiles validated")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
