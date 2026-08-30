#!/usr/bin/env python3
"""Validate the repository-owned v0 gym behavior inventory."""

from __future__ import annotations

import json
from pathlib import Path


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


def fail(message: str) -> None:
    raise SystemExit(f"gym inventory invalid: {message}")


def main() -> None:
    data = json.loads(INVENTORY.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        fail("schema_version must be 1")
    source = data.get("source", {})
    if source.get("frozen_schema_version") != 5:
        fail("frozen schema version must remain 5")
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
    commands = set(by_id["telegram.commands"]["surface"])
    if commands != REQUIRED_COMMANDS:
        fail(f"command drift: missing={sorted(REQUIRED_COMMANDS - commands)}, extra={sorted(commands - REQUIRED_COMMANDS)}")
    tools = set(by_id["mcp.tools"]["surface"])
    if tools != REQUIRED_MCP_TOOLS:
        fail(f"MCP drift: missing={sorted(REQUIRED_MCP_TOOLS - tools)}, extra={sorted(tools - REQUIRED_MCP_TOOLS)}")
    if by_id["telegram.free-text-agent"]["disposition"] != "blocked_d23":
        fail("agent-dependent free text must remain blocked by D23")
    if by_id["cutover.production"]["disposition"] != "deferred":
        fail("production cutover must remain deferred")

    print(
        f"gym inventory valid: {len(behaviors)} behaviors, "
        f"{len(REQUIRED_COMMANDS)} commands, {len(REQUIRED_MCP_TOOLS)} MCP tools"
    )


if __name__ == "__main__":
    main()
