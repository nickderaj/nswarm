#!/usr/bin/env python3
"""Reproducible, fail-closed Hermes gateway architecture-gate harness."""

from __future__ import annotations

import argparse
import ast
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tomllib
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
PIN_PATH = ROOT / "spikes" / "hermes" / "pin.json"


class SpikeError(ValueError):
    """A pinned source or architecture contract did not match."""


def load_pin(path: Path = PIN_PATH) -> dict[str, Any]:
    document = json.loads(path.read_text(encoding="utf-8"))
    required = {
        "package",
        "package_version",
        "python_requires",
        "repository",
        "tag",
        "tag_object_sha",
        "commit_sha",
        "plan_source_commit_prefix",
        "source_files",
    }
    if set(document) != required:
        raise SpikeError("pin schema differs")
    if document["package"] != "hermes-agent":
        raise SpikeError("unexpected package")
    if not isinstance(document["source_files"], dict) or not document["source_files"]:
        raise SpikeError("source file pins are missing")
    for name in ("tag_object_sha", "commit_sha"):
        value = document[name]
        if not isinstance(value, str) or len(value) != 40:
            raise SpikeError(f"{name} must be a full Git SHA")
    return document


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git_output(source: Path, *arguments: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(source), *arguments],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    if result.returncode != 0:
        raise SpikeError(result.stderr.strip() or "Git source check failed")
    return result.stdout.strip()


def class_method(tree: ast.Module, class_name: str, method_name: str) -> ast.FunctionDef | ast.AsyncFunctionDef:
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == class_name:
            for child in node.body:
                if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)) and child.name == method_name:
                    return child
    raise SpikeError(f"missing {class_name}.{method_name}")


def function(tree: ast.Module, name: str) -> ast.FunctionDef | ast.AsyncFunctionDef:
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == name:
            return node
    raise SpikeError(f"missing function {name}")


def calls_attribute(node: ast.AST, owner: str, attribute: str) -> list[int]:
    lines: list[int] = []
    for child in ast.walk(node):
        if not isinstance(child, ast.Call) or not isinstance(child.func, ast.Attribute):
            continue
        if child.func.attr != attribute:
            continue
        if isinstance(child.func.value, ast.Name) and child.func.value.id == owner:
            lines.append(child.lineno)
    return sorted(lines)


def references_name(node: ast.AST, name: str) -> list[int]:
    return sorted(
        child.lineno
        for child in ast.walk(node)
        if isinstance(child, ast.Name) and child.id == name
    )


def verify_source(source: Path, pin: dict[str, Any]) -> dict[str, Any]:
    source = source.resolve()
    if not source.is_dir():
        raise SpikeError("source is not a directory")
    head = git_output(source, "rev-parse", "HEAD")
    if head != pin["commit_sha"]:
        raise SpikeError(f"source commit {head} differs from pin {pin['commit_sha']}")
    tag = git_output(source, "describe", "--tags", "--exact-match")
    if tag != pin["tag"]:
        raise SpikeError(f"source tag {tag!r} differs from pin {pin['tag']!r}")

    mismatches = []
    for relative, expected in sorted(pin["source_files"].items()):
        path = source / relative
        actual = sha256(path) if path.is_file() else "missing"
        if actual != expected:
            mismatches.append(f"{relative}: {actual} != {expected}")
    if mismatches:
        raise SpikeError("source hash mismatch: " + "; ".join(mismatches))

    project = tomllib.loads((source / "pyproject.toml").read_text(encoding="utf-8"))["project"]
    for field, expected in (
        ("name", pin["package"]),
        ("version", pin["package_version"]),
        ("requires-python", pin["python_requires"]),
    ):
        if project.get(field) != expected:
            raise SpikeError(f"pyproject {field} differs from pin")

    api_path = source / "gateway" / "platforms" / "api_server.py"
    api_tree = ast.parse(api_path.read_text(encoding="utf-8"), filename=str(api_path))
    chat = class_method(api_tree, "APIServerAdapter", "_handle_session_chat")
    run_agent = class_method(api_tree, "APIServerAdapter", "_run_agent")
    chat_run_calls = calls_attribute(chat, "self", "_run_agent")
    create_calls = calls_attribute(run_agent, "self", "_create_agent")
    if len(chat_run_calls) != 1:
        raise SpikeError("session chat must call _run_agent exactly once")
    if len(create_calls) != 1:
        raise SpikeError("_run_agent must call _create_agent exactly once")
    if any(
        isinstance(node, ast.Attribute) and node.attr == "_agent_cache"
        for node in ast.walk(api_tree)
    ):
        raise SpikeError("API adapter unexpectedly acquired an agent cache")

    gateway_path = source / "gateway" / "run.py"
    gateway_text = gateway_path.read_text(encoding="utf-8")
    gateway_tree = ast.parse(gateway_text, filename=str(gateway_path))
    start_gateway = function(gateway_tree, "start_gateway")
    mcp_discovery_references = references_name(start_gateway, "discover_mcp_tools")
    if len(mcp_discovery_references) != 1 or "self._agent_cache" not in gateway_text:
        raise SpikeError("native gateway warm-cache/MCP startup contract differs")

    init_path = source / "agent" / "agent_init.py"
    init_text = init_path.read_text(encoding="utf-8")
    for anchor in (
        "agent.tools = _ra().get_tool_definitions(",
        "agent._cached_system_prompt: Optional[str] = None",
        "agent._todo_store = TodoStore()",
        "agent._memory_store.load_from_disk()",
    ):
        if anchor not in init_text:
            raise SpikeError(f"agent initialization anchor missing: {anchor}")

    conversation_path = source / "agent" / "conversation_loop.py"
    conversation_text = conversation_path.read_text(encoding="utf-8")
    for anchor in (
        "def _restore_or_build_system_prompt(",
        "agent._cached_system_prompt = stored_prompt",
        "agent._cached_system_prompt = agent._build_system_prompt(system_message)",
    ):
        if anchor not in conversation_text:
            raise SpikeError(f"prompt persistence anchor missing: {anchor}")

    return {
        "schema_version": 1,
        "pin": {
            "repository": pin["repository"],
            "tag": tag,
            "commit_sha": head,
            "package": project["name"],
            "package_version": project["version"],
            "python_requires": project["requires-python"],
        },
        "architecture_contract": {
            "session_chat_calls_run_agent": {"count": 1, "line": chat_run_calls[0]},
            "run_agent_calls_create_agent": {"count": 1, "line": create_calls[0]},
            "api_agent_cache_present": False,
            "native_gateway_agent_cache_present": True,
            "gateway_startup_mcp_discovery": {
                "count": 1,
                "executor_line": mcp_discovery_references[0],
            },
            "agent_tool_snapshot_per_construction": True,
            "agent_memory_load_per_construction_when_enabled": True,
            "agent_todo_state_per_construction": True,
            "prompt_full_build_first_turn": True,
            "prompt_restored_from_session_db_later_turns": True,
            "warm_agent_reuse_on_http_session_route": False,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    verify = subparsers.add_parser("verify-source")
    verify.add_argument("--source", required=True, type=Path)
    verify.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = verify_source(args.source, load_pin())
    except (OSError, SpikeError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        print(f"hermes spike: {error}", file=sys.stderr)
        return 1
    rendered = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
