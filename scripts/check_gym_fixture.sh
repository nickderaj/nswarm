#!/usr/bin/env bash
set -euo pipefail

temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/nswarm-gym-fixture.XXXXXX")
cleanup() {
  rm -rf "$temporary_dir"
}
trap cleanup EXIT

./scripts/generate_gym_fixture.sh "$temporary_dir/v0-gym-v5.sqlite3"

snapshot() {
  local database=$1
  sqlite3 "$database" 'PRAGMA user_version; PRAGMA integrity_check;'
  sqlite3 "$database" .dump
}

diff -u \
  <(snapshot fixtures/gym/v0-gym-v5.sqlite3) \
  <(snapshot "$temporary_dir/v0-gym-v5.sqlite3")
