#!/usr/bin/env python3
"""Render a reviewable inventory of every cargo-vet exemption."""

from __future__ import annotations

import json
import subprocess
import sys
import tomllib
import urllib.request
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
USER_AGENT = "nswarm-supply-chain-audit/1.0 (github.com/nickderaj/nswarm)"


def command(*args: str) -> str:
    return subprocess.run(
        args,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def publisher(name: str, version: str) -> str | None:
    url = f"https://crates.io/api/v1/crates/{name}/{version}"
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        record = json.load(response)["version"].get("published_by")
    if not record:
        return None
    return record["login"]


def shortest_paths(graph: list[dict[str, object]]) -> dict[int, list[tuple[int, str]]]:
    paths: dict[int, list[tuple[int, str]]] = {}
    queue: deque[int] = deque()
    for index, node in enumerate(graph):
        if node["is_workspace_member"]:
            paths[index] = []
            queue.append(index)
    while queue:
        parent = queue.popleft()
        node = graph[parent]
        edges = [
            (child, "runtime") for child in node["normal_deps"]
        ] + [
            (child, "build") for child in node["build_deps"]
        ] + [
            (child, "dev") for child in node["dev_deps"]
        ]
        for child, kind in edges:
            candidate = [*paths[parent], (parent, kind)]
            if child not in paths or len(candidate) < len(paths[child]):
                paths[child] = candidate
                queue.append(child)
    return paths


def main() -> int:
    config = tomllib.loads((ROOT / "supply-chain/config.toml").read_text())
    exemptions = config.get("exemptions", {})
    graph = json.loads(
        command(
            "cargo",
            "vet",
            "dump-graph",
            "--locked",
            "--output-format=json",
            "--depth",
            "full",
        )
    )
    paths = shortest_paths(graph)
    by_identity = {(node["name"], node["version"]): index for index, node in enumerate(graph)}
    records: list[dict[str, str]] = []
    for name, entries in exemptions.items():
        for entry in entries:
            version = entry["version"]
            index = by_identity[(name, version)]
            node = graph[index]
            path = paths[index]
            first_kind = path[0][1] if path else "runtime"
            if len(path) == 1:
                dependency_kind = f"direct-{first_kind}"
            elif node["is_dev_only"]:
                dependency_kind = "transitive-dev"
            elif any(kind == "build" for _, kind in path):
                dependency_kind = "transitive-build"
            else:
                dependency_kind = "transitive-runtime"
            names = [graph[parent]["name"] for parent, _ in path] + [name]
            if dependency_kind == "direct-runtime" or name == "libsqlite3-sys":
                debt_class = "P1 direct/security-sensitive"
            elif dependency_kind in {"direct-dev", "transitive-dev", "transitive-build"}:
                debt_class = "P3 build/dev-only"
            else:
                debt_class = "P2 transitive bootstrap"
            records.append(
                {
                    "name": name,
                    "version": version,
                    "criterion": entry["criteria"],
                    "kind": dependency_kind,
                    "path": " → ".join(names),
                    "class": debt_class,
                }
            )

    with ThreadPoolExecutor(max_workers=8) as executor:
        publishers = list(
            executor.map(lambda item: publisher(item["name"], item["version"]), records)
        )
    counts: dict[str, int] = {}
    for record in records:
        counts[record["class"]] = counts.get(record["class"], 0) + 1
    generated = datetime.now(UTC).date().isoformat()
    print("# Supply-chain exemption inventory\n")
    print(
        f"Generated {generated} from the locked Cargo graph, `supply-chain/config.toml`, "
        "and crates.io's exact-version API. The version link is the registry source; "
        "publisher is the account attached to that exact release. `not exposed` means "
        "the exact-version API returned no publishing account, not that publisher "
        "identity was inferred. Paths are shortest representative dependency paths; "
        "a crate can have additional consumers.\n"
    )
    print(
        f"All {len(records)} entries remain exemptions, not audits: "
        f"{counts.get('P1 direct/security-sensitive', 0)} direct/security-sensitive, "
        f"{counts.get('P2 transitive bootstrap', 0)} transitive bootstrap, and "
        f"{counts.get('P3 build/dev-only', 0)} build/dev-only. No exemption was "
        "converted to an audit without source-review evidence.\n"
    )
    print("## Locked inventory\n")
    print("| Crate | Version/source | Publisher | Kind | Use/path | Criterion | Review class |")
    print("|---|---|---|---|---|---|---|")
    for record, login in zip(records, publishers, strict=True):
        source = f"[{record['version']}](https://crates.io/crates/{record['name']}/{record['version']})"
        publisher_link = (
            f"[@{login}](https://crates.io/users/{login})" if login else "not exposed"
        )
        print(
            f"| `{record['name']}` | {source} | {publisher_link} | "
            f"{record['kind']} | {record['path']} | `{record['criterion']}` | "
            f"{record['class']} |"
        )
    print("\n## Disposition and assigned follow-up\n")
    print(
        "- **P1 — repository maintainer:** review direct runtime and native SQLite "
        "boundary crates first. Record a cargo-vet audit only after examining the exact "
        "source/version and satisfying the named criterion; otherwise keep the exemption.\n"
    )
    print(
        "- **P2 — repository maintainer:** seek trustworthy upstream audit imports or "
        "perform exact-version source review after P1. These are explicit bootstrap debt, "
        "not evidence that the crates were audited locally.\n"
    )
    print(
        "- **P3 — repository maintainer:** batch build/dev-only review after deploy-path "
        "dependencies. Native build tooling still affects produced artifacts even when it "
        "does not execute in the deployed service.\n"
    )
    print(
        "`cargo vet check` remains mandatory in `just ci`; a newly resolved third-party "
        "version has neither an exemption nor an audit and therefore fails closed. "
        "Publisher identity is context for prioritization only and is never treated as "
        "proof of safety."
    )
    print(f"\nTotal exemptions: {len(records)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
