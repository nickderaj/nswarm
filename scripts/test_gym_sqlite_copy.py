#!/usr/bin/env python3
"""Hermetic tests for the disposable gym database copy boundary."""

from __future__ import annotations

import json
import os
import sqlite3
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/copy_gym_database.sh"
SCHEMA = ROOT / "fixtures/gym/v0-gym-v5.sql"


class GymSqliteCopyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.source = self.root / "source gym.db"
        with sqlite3.connect(self.source) as connection:
            connection.executescript(SCHEMA.read_text(encoding="utf-8"))
            connection.execute(
                "INSERT INTO body_metrics (date, metric, value, unit, source) VALUES (?, ?, ?, ?, ?)",
                ("2026-08-30T09:30:00+01:00", "weight_kg", 80.5, "kg", "manual"),
            )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_copy(self, source: Path, destination: Path, *extra: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [str(SCRIPT), str(source), str(destination), *extra],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_consistent_copy_is_validated_and_records_safe_metadata(self) -> None:
        destination = self.root / "trial gym.db"
        metadata_path = self.root / "trial metadata.json"
        result = self.run_copy(
            self.source, destination, "--metadata", str(metadata_path)
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        self.assertEqual(metadata["destination_filename"], destination.name)
        self.assertTrue(metadata["disposable_parallel_trial"])
        self.assertEqual(metadata["schema_version"], 5)
        self.assertEqual(metadata["integrity"], "ok")
        self.assertEqual(metadata["foreign_key_violations"], 0)
        self.assertEqual(len(metadata["sha256"]), 64)
        self.assertNotIn(str(self.source), metadata_path.read_text(encoding="utf-8"))
        with sqlite3.connect(destination) as connection:
            self.assertEqual(connection.execute("PRAGMA integrity_check").fetchone()[0], "ok")
            self.assertEqual(connection.execute("SELECT count(*) FROM body_metrics").fetchone()[0], 1)

    def test_online_backup_includes_uncheckpointed_wal_commit_without_sidecars(self) -> None:
        with sqlite3.connect(self.source) as connection:
            connection.execute("PRAGMA journal_mode=WAL")
            connection.execute(
                "INSERT INTO body_metrics (date, metric, value, unit, source) VALUES (?, ?, ?, ?, ?)",
                ("2026-08-30T10:30:00+01:00", "weight_kg", 81.0, "kg", "manual"),
            )
            connection.commit()
            destination = self.root / "wal-copy.db"
            result = self.run_copy(self.source, destination)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(Path(f"{destination}-wal").exists())
            self.assertFalse(Path(f"{destination}-shm").exists())
            with sqlite3.connect(destination) as copied:
                self.assertEqual(copied.execute("SELECT count(*) FROM body_metrics").fetchone()[0], 2)

    def test_refuses_existing_missing_and_wrong_schema_inputs(self) -> None:
        destination = self.root / "existing.db"
        destination.touch()
        self.assertEqual(self.run_copy(self.source, destination).returncode, 2)
        missing_destination = self.root / "missing-copy.db"
        self.assertEqual(
            self.run_copy(self.root / "missing.db", missing_destination).returncode, 2
        )
        self.assertFalse(missing_destination.exists())
        wrong = self.root / "wrong.db"
        with sqlite3.connect(wrong) as connection:
            connection.execute("PRAGMA user_version=4")
        wrong_destination = self.root / "wrong-copy.db"
        self.assertEqual(self.run_copy(wrong, wrong_destination).returncode, 2)
        self.assertFalse(wrong_destination.exists())

    def test_refuses_lexical_symlink_hardlink_and_sidecar_aliases(self) -> None:
        self.assertEqual(self.run_copy(self.source, self.source).returncode, 2)
        symlink = self.root / "source-link.db"
        symlink.symlink_to(self.source)
        self.assertEqual(self.run_copy(self.source, symlink).returncode, 2)
        hardlink = self.root / "source-hardlink.db"
        os.link(self.source, hardlink)
        self.assertEqual(self.run_copy(self.source, hardlink).returncode, 2)
        self.assertEqual(
            self.run_copy(self.source, Path(f"{self.source}-wal")).returncode, 2
        )

    def test_refuses_foreign_key_violations_and_existing_metadata(self) -> None:
        invalid = self.root / "invalid.db"
        with sqlite3.connect(invalid) as connection:
            connection.executescript(SCHEMA.read_text(encoding="utf-8"))
            connection.execute("PRAGMA foreign_keys=OFF")
            connection.execute(
                "INSERT INTO efforts (session_item_id, position) VALUES (999, 1)"
            )
        invalid_destination = self.root / "invalid-copy.db"
        self.assertEqual(self.run_copy(invalid, invalid_destination).returncode, 2)
        self.assertFalse(invalid_destination.exists())

        metadata = self.root / "metadata.json"
        metadata.write_text("preserve", encoding="utf-8")
        destination = self.root / "metadata-copy.db"
        result = self.run_copy(
            self.source, destination, "--metadata", str(metadata)
        )
        self.assertEqual(result.returncode, 2)
        self.assertFalse(destination.exists())
        self.assertEqual(metadata.read_text(encoding="utf-8"), "preserve")


if __name__ == "__main__":
    unittest.main()
