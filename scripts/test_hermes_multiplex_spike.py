#!/usr/bin/env python3
"""Tests for the local-only D24 Hermes multiplexing harness."""

from __future__ import annotations

from argparse import Namespace
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "hermes_multiplex_spike", ROOT / "scripts" / "hermes_multiplex_spike.py"
)
assert SPEC is not None and SPEC.loader is not None
SPIKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SPIKE)


class ArgumentTests(unittest.TestCase):
    def args(self, directory: Path, **overrides: object) -> Namespace:
        values: dict[str, object] = {
            "source": directory,
            "python": Path("/bin/sh"),
            "output": SPIKE.DEFAULT_OUTPUT,
            "workers": 4,
            "i_understand_this_is_local_only": True,
        }
        values.update(overrides)
        return Namespace(**values)

    def test_requires_explicit_local_only_acknowledgement(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(SPIKE.MultiplexSpikeError, "local-only"):
                SPIKE.validate_args(
                    self.args(
                        Path(directory),
                        i_understand_this_is_local_only=False,
                    )
                )

    def test_caps_test_workers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(SPIKE.MultiplexSpikeError, "between 1 and 4"):
                SPIKE.validate_args(self.args(Path(directory), workers=5))

    def test_rejects_output_outside_evidence_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            with self.assertRaisesRegex(SPIKE.MultiplexSpikeError, "directly under"):
                SPIKE.validate_args(self.args(path, output=path / "raw.json"))

    def test_preserves_virtualenv_interpreter_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            interpreter = root / "python"
            interpreter.symlink_to("/bin/sh")
            _, actual, _ = SPIKE.validate_args(self.args(root, python=interpreter))
            self.assertEqual(actual, Path(os.path.abspath(interpreter)))


class SummaryTests(unittest.TestCase):
    def test_parses_passing_canonical_summary(self) -> None:
        output = (
            "=== Summary: 27 files, 288 tests passed, 0 failed, 2 skipped "
            "(100% complete) in 5.3s (4 workers) ==="
        )
        summary = SPIKE.parse_summary(output, 4)
        self.assertEqual(summary["passed"], 288)
        self.assertEqual(summary["wall_ms"], 5_300)

    def test_rejects_failed_suite(self) -> None:
        output = (
            "=== Summary: 27 files, 287 tests passed, 1 failed, 2 skipped "
            "(100% complete) in 5.3s (4 workers) ==="
        )
        with self.assertRaisesRegex(SPIKE.MultiplexSpikeError, "did not pass"):
            SPIKE.parse_summary(output, 4)

    def test_rejects_flaky_retry(self) -> None:
        output = (
            "=== Summary: 27 files, 288 tests passed, 0 failed, 2 skipped "
            "(100% complete) in 5.3s (4 workers) ===\n=== ⚠ 1 FLAKY file ==="
        )
        with self.assertRaisesRegex(SPIKE.MultiplexSpikeError, "required a retry"):
            SPIKE.parse_summary(output, 4)


if __name__ == "__main__":
    unittest.main()
