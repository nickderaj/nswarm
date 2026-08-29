#!/usr/bin/env bash
set -euo pipefail

temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/nswarm-gym-fixture.XXXXXX")
cleanup() {
  rm -rf "$temporary_dir"
}
trap cleanup EXIT

./scripts/generate_gym_fixture.sh "$temporary_dir/v0-gym-v5.sqlite3"
cmp fixtures/gym/v0-gym-v5.sqlite3 "$temporary_dir/v0-gym-v5.sqlite3"
