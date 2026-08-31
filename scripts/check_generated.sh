#!/usr/bin/env bash
set -euo pipefail

temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/nswarm-generated.XXXXXX")
cleanup() {
  rm -rf "$temporary_dir"
}
trap cleanup EXIT

./scripts/generate.sh "$temporary_dir/generated"
diff -ru generated/systemd "$temporary_dir/generated/systemd"
diff -ru generated/tmpfiles "$temporary_dir/generated/tmpfiles"
./scripts/check_gym_fixture.sh

tree_status=$(git status --porcelain=v1 --untracked-files=all -- generated profiles fixtures/gym)
if [[ -n "$tree_status" ]]; then
  echo "generated/profile tree is not clean:" >&2
  echo "$tree_status" >&2
  exit 1
fi
