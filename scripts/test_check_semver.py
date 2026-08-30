#!/usr/bin/env python3
"""Regression tests for fail-closed semver package classification."""

from __future__ import annotations

import unittest

from scripts.check_semver import (
    MSRV_RESOLVER_ENV,
    package_delta,
    semver_environment,
    workspace_package_names,
)


class PackageDeltaTests(unittest.TestCase):
    """Package identity, rather than manifest path, controls baseline matching."""

    def test_new_package_is_excluded_from_its_initial_baseline(self) -> None:
        self.assertEqual(
            package_delta({"agent-control", "gym-bot"}, {"agent-control"}),
            (["gym-bot"], []),
        )

    def test_moved_package_retains_its_identity(self) -> None:
        self.assertEqual(package_delta({"gym-bot"}, {"gym-bot"}), ([], []))

    def test_renamed_package_exposes_both_sides_of_the_change(self) -> None:
        self.assertEqual(
            package_delta({"gym-service"}, {"gym-bot"}),
            (["gym-service"], ["gym-bot"]),
        )

    def test_removed_package_fails_the_removed_identity_check(self) -> None:
        self.assertEqual(
            package_delta({"agent-control"}, {"agent-control", "botkit"}),
            ([], ["botkit"]),
        )

    def test_duplicate_workspace_names_are_rejected(self) -> None:
        metadata = {
            "workspace_members": ["first", "second"],
            "packages": [
                {"id": "first", "name": "duplicate"},
                {"id": "second", "name": "duplicate"},
            ],
        }
        with self.assertRaisesRegex(ValueError, "must be unique"):
            workspace_package_names(metadata)

    def test_semver_scratch_packages_respect_the_workspace_msrv(self) -> None:
        environment = semver_environment(
            {"PATH": "/toolchain/bin", MSRV_RESOLVER_ENV: "allow"}
        )
        self.assertEqual(environment[MSRV_RESOLVER_ENV], "fallback")
        self.assertEqual(environment["PATH"], "/toolchain/bin")


if __name__ == "__main__":
    unittest.main()
