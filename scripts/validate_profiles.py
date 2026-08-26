#!/usr/bin/env python3
"""Validate root-owned profile sources and their canonical generated forms."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent
EXPECTED = {
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
        "capabilities": {"repository-read", "network-read", "evidence-write"},
        "forbidden": {"repository-write", "branch-push", "integrate", "merge", "deploy"},
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
        "capabilities": {"repository-read", "repository-write", "evidence-write", "branch-push"},
        "forbidden": {"coordinate", "verify", "integrate", "merge", "deploy"},
    },
}


def fail(message: str) -> None:
    raise ValueError(message)


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


def validate_profile(name: str, expected: dict[str, set[str]]) -> None:
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
    if set(profile["capabilities"]) != expected["capabilities"]:
        fail(f"{name}: structural capability drift")
    if set(profile["forbidden_capabilities"]) != expected["forbidden"]:
        fail(f"{name}: forbidden capability drift")
    if profile["governance"] != {
        "write_approval": True,
        "background_review": False,
        "curator": False,
    }:
        fail(f"{name}: governance must fail closed")
    if profile["network"] != {"default": "deny", "brief_allow_list_required": True}:
        fail(f"{name}: network policy must default deny")

    soul_path = ROOT / profile["soul"]
    soul = soul_path.read_text(encoding="utf-8").lower()
    for phrase in ("immutable", "untrusted attributed", "cannot", "evidence"):
        if phrase not in soul:
            fail(f"{name}: SOUL missing required mechanism {phrase!r}")

    skill_root = ROOT / profile["skills"]
    actual_skills = {path.parent.name for path in skill_root.glob("*/SKILL.md")}
    if actual_skills != expected["skills"]:
        fail(f"{name}: skill inventory drift")
    for skill in sorted(actual_skills):
        metadata = parse_frontmatter(skill_root / skill / "SKILL.md")
        if metadata["name"] != skill:
            fail(f"{name}/{skill}: name must match directory")

    validate_memory(ROOT / profile["memory"] / "MEMORY.md")

    canonical = json.dumps(profile, indent=2, sort_keys=True) + "\n"
    generated_path = ROOT / "generated" / "profiles" / f"{name}.json"
    if not generated_path.exists() or generated_path.read_text(encoding="utf-8") != canonical:
        fail(f"{generated_path.relative_to(ROOT)}: generated profile drift")
    if json.loads(canonical) != profile:
        fail(f"{name}: canonical profile does not round-trip")


def main() -> int:
    try:
        for profile_name, expected in EXPECTED.items():
            validate_profile(profile_name, expected)
    except (OSError, ValueError, tomllib.TOMLDecodeError, json.JSONDecodeError) as error:
        print(f"profile policy: {error}", file=sys.stderr)
        return 1
    print(f"profile policy: {len(EXPECTED)} profiles validated")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
