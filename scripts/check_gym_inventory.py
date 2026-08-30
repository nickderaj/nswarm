#!/usr/bin/env python3
"""Validate the repository-owned v0 gym behavior inventory."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "docs/gym/v0-behavior-inventory.json"
ALLOWED_DISPOSITIONS = {"step2", "ported", "changed_d7", "blocked_d23", "deferred"}
REQUIRED_CATEGORIES = {
    "agent",
    "database",
    "exports",
    "fleet",
    "health",
    "mcp",
    "profile",
    "runtime",
    "scheduler",
    "telegram",
}
REQUIRED_COMMANDS = {
    "gym",
    "cardio",
    "run",
    "weight",
    "batch",
    "plan",
    "plans",
    "rate",
    "adherence",
    "cost",
    "sync",
    "export",
    "import_zip",
    "preference",
    "help",
}
REQUIRED_MCP_TOOLS = {
    "recent_sets",
    "exercise_catalogue",
    "volume_summary",
    "body_metrics",
    "recent_runs",
    "pace_trend",
    "interval_history",
    "heart_rate_series",
    "weekly_load",
    "preferences",
    "record_preference",
    "propose_plan",
    "plan_feedback",
}
SOURCE_REF_LINE_SUFFIX = re.compile(r":\d+(?:-\d+)?$")
LOWER_HEX = frozenset("0123456789abcdef")


def fail(message: str) -> None:
    raise SystemExit(f"gym inventory invalid: {message}")


def require_behavior(
    by_id: dict[str, dict[str, object]], identifier: str
) -> dict[str, object]:
    behavior = by_id.get(identifier)
    if behavior is None:
        fail(f"required behavior is missing: {identifier}")
    return behavior


def is_sha(value: object, length: int) -> bool:
    return (
        isinstance(value, str)
        and len(value) == length
        and all(character in LOWER_HEX for character in value)
    )


def surface_digest(values: set[str]) -> str:
    canonical = json.dumps(sorted(values), separators=(",", ":"), ensure_ascii=True)
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source-root",
        type=Path,
        help="optional checkout of the pinned ultron commit to rehash",
    )
    return parser.parse_args()


def main() -> None:
    arguments = parse_arguments()
    data = json.loads(INVENTORY.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        fail("schema_version must be 1")
    source = data.get("source", {})
    if source.get("frozen_schema_version") != 5:
        fail("frozen schema version must remain 5")
    if not is_sha(source.get("commit_sha"), 40):
        fail("source.commit_sha must be a full lowercase Git SHA")
    source_files = source.get("source_files")
    if not isinstance(source_files, dict) or not source_files:
        fail("source.source_files must contain the frozen source pins")
    for path, digest in source_files.items():
        if not isinstance(path, str) or not path or not is_sha(digest, 64):
            fail(f"invalid SHA-256 source pin: {path!r}")
    surface_pins = source.get("surface_sha256")
    if not isinstance(surface_pins, dict):
        fail("source.surface_sha256 must pin reviewed surface lists")
    if set(surface_pins) != {"telegram_commands", "mcp_tools"}:
        fail("source.surface_sha256 keys differ from the reviewed surfaces")
    for name, digest in surface_pins.items():
        if not is_sha(digest, 64):
            fail(f"invalid SHA-256 surface pin: {name}")
    behaviors = data.get("behaviors")
    if not isinstance(behaviors, list) or not behaviors:
        fail("behaviors must be a non-empty list")

    ids: set[str] = set()
    categories: set[str] = set()
    by_id: dict[str, dict[str, object]] = {}
    for behavior in behaviors:
        if not isinstance(behavior, dict):
            fail("every behavior must be an object")
        identifier = behavior.get("id")
        if not isinstance(identifier, str) or not identifier:
            fail("every behavior needs a non-empty id")
        if identifier in ids:
            fail(f"duplicate behavior id: {identifier}")
        ids.add(identifier)
        by_id[identifier] = behavior
        category = behavior.get("category")
        if not isinstance(category, str) or not category:
            fail(f"{identifier} needs a category")
        categories.add(category)
        disposition = behavior.get("disposition")
        if disposition not in ALLOWED_DISPOSITIONS:
            fail(f"{identifier} has invalid disposition: {disposition}")
        for field in ("surface", "source_refs"):
            values = behavior.get(field)
            if not isinstance(values, list) or not values or not all(
                isinstance(value, str) and value for value in values
            ):
                fail(f"{identifier}.{field} must be a non-empty string list")
        if not isinstance(behavior.get("reason"), str) or not behavior["reason"]:
            fail(f"{identifier} needs an explicit reason")

    missing_categories = REQUIRED_CATEGORIES - categories
    if missing_categories:
        fail(f"missing categories: {sorted(missing_categories)}")
    referenced_files = {
        SOURCE_REF_LINE_SUFFIX.sub("", reference)
        for behavior in behaviors
        for reference in behavior["source_refs"]
    }
    if set(source_files) != referenced_files:
        fail(
            "source pin coverage drift: "
            f"missing={sorted(referenced_files - set(source_files))}, "
            f"extra={sorted(set(source_files) - referenced_files)}"
        )

    command_behavior = require_behavior(by_id, "telegram.commands")
    zip_behavior = require_behavior(by_id, "health.zip-backfill")
    commands = (
        set(command_behavior["surface"]) | set(zip_behavior["surface"])
    ) & REQUIRED_COMMANDS
    if commands != REQUIRED_COMMANDS:
        fail(
            f"command drift: missing={sorted(REQUIRED_COMMANDS - commands)}, "
            f"extra={sorted(commands - REQUIRED_COMMANDS)}"
        )
    tools = set(require_behavior(by_id, "mcp.tools")["surface"])
    if tools != REQUIRED_MCP_TOOLS:
        fail(
            f"MCP drift: missing={sorted(REQUIRED_MCP_TOOLS - tools)}, "
            f"extra={sorted(tools - REQUIRED_MCP_TOOLS)}"
        )
    if surface_pins["telegram_commands"] != surface_digest(commands):
        fail("Telegram command surface differs from its reviewed SHA-256 pin")
    if surface_pins["mcp_tools"] != surface_digest(tools):
        fail("MCP tool surface differs from its reviewed SHA-256 pin")
    if require_behavior(by_id, "telegram.approval-callback")["disposition"] not in {
        "ported",
        "deferred",
    }:
        fail("the Keep/Reject approval callback must remain ported or transport-deferred")
    if require_behavior(by_id, "telegram.free-text-agent")["disposition"] != "blocked_d23":
        fail("agent-dependent free text must remain blocked by D23")
    if require_behavior(by_id, "cutover.production")["disposition"] != "deferred":
        fail("production cutover must remain deferred")

    if arguments.source_root is not None:
        for relative, expected in source_files.items():
            path = arguments.source_root / relative
            if not path.is_file():
                fail(f"pinned source file is missing: {relative}")
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            if actual != expected:
                fail(f"pinned source file changed: {relative}")

    print(
        f"gym inventory valid: {len(behaviors)} behaviors, "
        f"{len(REQUIRED_COMMANDS)} commands, {len(REQUIRED_MCP_TOOLS)} MCP tools"
    )


if __name__ == "__main__":
    main()
