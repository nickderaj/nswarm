#!/usr/bin/env python3
"""Reproducible, fail-closed Hermes gateway architecture-gate harness."""

from __future__ import annotations

import argparse
import ast
import asyncio
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time
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
        "capabilities_contract_sha256",
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
    if len(document["capabilities_contract_sha256"]) != 64:
        raise SpikeError("capabilities contract must be a SHA-256 digest")
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


def latency_summary(samples: list[float]) -> dict[str, float]:
    if not samples:
        raise SpikeError("latency samples are empty")
    ordered = sorted(samples)
    p95_index = max(0, (95 * len(ordered) + 99) // 100 - 1)
    return {
        "minimum_ms": round(ordered[0], 3),
        "median_ms": round(statistics.median(ordered), 3),
        "mean_ms": round(statistics.fmean(ordered), 3),
        "p95_ms": round(ordered[p95_index], 3),
        "maximum_ms": round(ordered[-1], 3),
    }


async def measure_http_reuse(source: Path, samples: int) -> dict[str, Any]:
    if samples < 3:
        raise SpikeError("at least three samples are required")
    source_contract = verify_source(source, load_pin())
    sys.path.insert(0, str(source.resolve()))

    try:
        from aiohttp import web
        from aiohttp.test_utils import TestClient, TestServer
        from gateway.config import PlatformConfig
        import gateway.platforms.api_server as api_module
        from gateway.platforms.api_server import APIServerAdapter
        from hermes_state import SessionDB
    except ImportError as error:
        raise SpikeError(
            "measurement requires the pinned Hermes environment; run with its Python"
        ) from error

    token = "local-spike-auth-token-00000000"
    headers = {"Authorization": f"Bearer {token}"}
    created: list[dict[str, Any]] = []
    histories: list[int] = []

    class FakeLocalProviderAgent:
        session_prompt_tokens = 0
        session_completion_tokens = 0
        session_total_tokens = 0

        def __init__(self, session_id: str):
            self.session_id = session_id
            self.instance_number = len(created) + 1
            self._last_compaction_in_place = False
            created.append(
                {
                    "instance_number": self.instance_number,
                    "session_id": session_id,
                }
            )

        def run_conversation(
            self,
            user_message: str,
            conversation_history: list[dict[str, Any]],
            task_id: str,
        ) -> dict[str, Any]:
            del user_message, task_id
            histories.append(len(conversation_history))
            return {
                "final_response": "deterministic local provider response",
                "messages": [],
                "session_id": self.session_id,
            }

    def fake_create_agent(**kwargs: Any) -> FakeLocalProviderAgent:
        return FakeLocalProviderAgent(kwargs["session_id"])

    old_publish = api_module._publish_turn_process_ownership
    old_clear = api_module._clear_turn_process_ownership
    api_module._publish_turn_process_ownership = lambda agent, task_id: None
    api_module._clear_turn_process_ownership = lambda agent: None

    prior_home = os.environ.get("HERMES_HOME")
    with tempfile.TemporaryDirectory(prefix="nswarm-hermes-spike-") as directory:
        home = Path(directory)
        os.environ["HERMES_HOME"] = str(home)
        database = SessionDB(home / "state.db")
        adapter = APIServerAdapter(
            PlatformConfig(
                enabled=True,
                extra={"host": "127.0.0.1", "port": 0, "key": token},
            )
        )
        adapter._session_db = database
        adapter._create_agent = fake_create_agent

        app = web.Application()
        app.router.add_get("/v1/capabilities", adapter._handle_capabilities)
        app.router.add_post("/api/sessions", adapter._handle_create_session)
        app.router.add_post(
            "/api/sessions/{session_id}/chat", adapter._handle_session_chat
        )

        async def create_session(client: TestClient, session_id: str) -> None:
            response = await client.post(
                "/api/sessions",
                headers=headers,
                json={"id": session_id, "source": "nswarm_step3_spike"},
            )
            if response.status != 201:
                raise SpikeError(
                    f"session create failed ({response.status}): {await response.text()}"
                )

        async def chat(client: TestClient, session_id: str) -> float:
            started = time.perf_counter_ns()
            response = await client.post(
                f"/api/sessions/{session_id}/chat",
                headers=headers,
                json={"message": "deterministic reuse probe"},
            )
            elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
            if response.status != 200:
                raise SpikeError(
                    f"session chat failed ({response.status}): {await response.text()}"
                )
            payload = await response.json()
            if payload.get("session_id") != session_id:
                raise SpikeError("explicit session ID was not preserved")
            return round(elapsed_ms, 3)

        try:
            async with TestClient(TestServer(app)) as client:
                capabilities_response = await client.get(
                    "/v1/capabilities", headers=headers
                )
                if capabilities_response.status != 200:
                    raise SpikeError("capabilities request failed")
                capabilities = await capabilities_response.json()
                capabilities_digest = hashlib.sha256(
                    json.dumps(
                        capabilities, sort_keys=True, separators=(",", ":")
                    ).encode()
                ).hexdigest()
                expected_digest = load_pin()["capabilities_contract_sha256"]
                if capabilities_digest != expected_digest:
                    raise SpikeError(
                        "pinned /v1/capabilities response differs from contract"
                    )

                route_prime_session = "route-prime-session"
                await create_session(client, route_prime_session)
                route_prime_ms = await chat(client, route_prime_session)

                cold_samples: list[float] = []
                for index in range(samples):
                    session_id = f"cold-session-{index:03d}"
                    await create_session(client, session_id)
                    cold_samples.append(await chat(client, session_id))

                warm_session = "explicit-warm-session"
                await create_session(client, warm_session)
                warm_prime_ms = await chat(client, warm_session)
                await asyncio.to_thread(
                    database.replace_messages,
                    warm_session,
                    [
                        {"role": "user", "content": "persisted probe"},
                        {"role": "assistant", "content": "persisted response"},
                    ],
                )
                warm_samples = [
                    await chat(client, warm_session) for _ in range(samples)
                ]
        finally:
            close = getattr(database, "close", None)
            if callable(close):
                close()
            api_module._publish_turn_process_ownership = old_publish
            api_module._clear_turn_process_ownership = old_clear
            if prior_home is None:
                os.environ.pop("HERMES_HOME", None)
            else:
                os.environ["HERMES_HOME"] = prior_home

    expected_constructions = samples * 2 + 2
    if len(created) != expected_constructions:
        raise SpikeError(
            f"expected {expected_constructions} agent constructions, got {len(created)}"
        )
    warm_instances = [
        item["instance_number"]
        for item in created
        if item["session_id"] == "explicit-warm-session"
    ]
    if len(warm_instances) != samples + 1 or len(set(warm_instances)) != samples + 1:
        raise SpikeError("warm-session requests did not construct distinct agents")

    return {
        "schema_version": 1,
        "measurement": "instrumented_http_session_route",
        "provider_boundary": "deterministic_local_fake",
        "claims_excluded": [
            "live provider latency",
            "provider-side prompt-cache latency",
            "Raspberry Pi latency",
        ],
        "environment": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "timer": "time.perf_counter_ns (monotonic)",
        },
        "pin": source_contract["pin"],
        "source_contract": source_contract["architecture_contract"],
        "capabilities": capabilities,
        "trial": {
            "cold_definition": "first chat request for a newly created explicit session",
            "warm_definition": "repeat chat request for one explicit session after one unrecorded prime",
            "samples_per_class": samples,
            "warm_prime_ms": round(warm_prime_ms, 3),
            "route_prime_ms": round(route_prime_ms, 3),
            "cold_raw_ms": cold_samples,
            "warm_raw_ms": warm_samples,
            "cold_summary": latency_summary(cold_samples),
            "warm_summary": latency_summary(warm_samples),
        },
        "construction_observations": {
            "chat_requests": expected_constructions,
            "agent_factory_calls": len(created),
            "distinct_agent_instances_for_warm_session": len(set(warm_instances)),
            "warm_session_chat_requests": samples + 1,
            "conversation_history_lengths": histories,
            "warm_agent_reused": False,
        },
        "decision": {
            "d23_http_warm_agent_gate": "failed",
            "reason": "every HTTP session-chat request constructs a fresh AIAgent",
            "next_action": "revisit D23 and section 6.3 before botkit conversation code",
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    verify = subparsers.add_parser("verify-source")
    verify.add_argument("--source", required=True, type=Path)
    verify.add_argument("--output", type=Path)
    measure = subparsers.add_parser("measure-reuse")
    measure.add_argument("--source", required=True, type=Path)
    measure.add_argument("--samples", type=int, default=10)
    measure.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "verify-source":
            result = verify_source(args.source, load_pin())
        else:
            result = asyncio.run(measure_http_reuse(args.source, args.samples))
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
