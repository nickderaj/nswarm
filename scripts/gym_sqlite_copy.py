#!/usr/bin/env python3
"""Create and validate a consistent disposable SQLite copy."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sqlite3
import sys
from pathlib import Path


EXPECTED_SCHEMA_VERSION = 5


def sqlite_family(path: Path) -> tuple[Path, Path, Path]:
    return path, Path(f"{path}-wal"), Path(f"{path}-shm")


def aliases(left: Path, right: Path) -> bool:
    try:
        return os.path.samefile(left, right)
    except FileNotFoundError:
        return left.resolve(strict=False) == right.resolve(strict=False)


def ensure_separate(source: Path, destination: Path) -> None:
    for source_member in sqlite_family(source):
        for destination_member in sqlite_family(destination):
            if aliases(source_member, destination_member):
                raise ValueError("source and destination SQLite families must not alias")


def validate(connection: sqlite3.Connection) -> dict[str, object]:
    version = int(connection.execute("PRAGMA user_version").fetchone()[0])
    if version != EXPECTED_SCHEMA_VERSION:
        raise ValueError(
            f"gym schema version mismatch: expected {EXPECTED_SCHEMA_VERSION}, found {version}"
        )
    integrity = [str(row[0]) for row in connection.execute("PRAGMA integrity_check")]
    if integrity != ["ok"]:
        raise ValueError(f"SQLite integrity check failed: {integrity}")
    foreign_keys = int(connection.execute("PRAGMA foreign_keys").fetchone()[0])
    connection.execute("PRAGMA foreign_keys=ON")
    violations = list(connection.execute("PRAGMA foreign_key_check"))
    if violations:
        raise ValueError(f"gym database has {len(violations)} foreign-key violation(s)")
    tables = int(
        connection.execute(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'"
        ).fetchone()[0]
    )
    return {
        "schema_version": version,
        "integrity": "ok",
        "foreign_keys_before_validation": bool(foreign_keys),
        "foreign_key_violations": 0,
        "application_tables": tables,
    }


def copy_database(source: Path, destination: Path) -> dict[str, object]:
    if not source.is_file():
        raise ValueError("source must be an existing regular file")
    if destination.exists():
        raise ValueError("destination already exists; refusing to overwrite")
    if not destination.parent.is_dir():
        raise ValueError("destination parent must be an existing directory")
    ensure_separate(source, destination)

    temporary = destination.with_name(f".{destination.name}.copy-{os.getpid()}")
    if temporary.exists():
        raise ValueError("script-owned temporary destination already exists")
    try:
        source_uri = f"file:{source.resolve()}?mode=ro"
        with sqlite3.connect(source_uri, uri=True) as source_connection:
            with sqlite3.connect(temporary) as destination_connection:
                source_connection.backup(destination_connection)
                metadata = validate(destination_connection)
                destination_connection.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        for sidecar in sqlite_family(temporary)[1:]:
            sidecar.unlink(missing_ok=True)
        os.replace(temporary, destination)
    finally:
        for member in sqlite_family(temporary):
            member.unlink(missing_ok=True)

    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    return {
        "disposable_parallel_trial": True,
        "destination_filename": destination.name,
        "bytes": destination.stat().st_size,
        "sha256": digest,
        **metadata,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Copy a gym SQLite database for a disposable v1 parallel trial"
    )
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument(
        "--metadata",
        type=Path,
        help="optional metadata JSON path; must not already exist",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.metadata is not None:
            if args.metadata.exists():
                raise ValueError("metadata destination already exists; refusing to overwrite")
            if not args.metadata.parent.is_dir():
                raise ValueError("metadata parent must be an existing directory")
        metadata = copy_database(args.source, args.destination)
        rendered = json.dumps(metadata, indent=2, sort_keys=True) + "\n"
        if args.metadata is None:
            sys.stdout.write(rendered)
        else:
            args.metadata.write_text(rendered, encoding="utf-8")
    except (OSError, sqlite3.Error, ValueError) as error:
        print(f"gym database copy refused: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
