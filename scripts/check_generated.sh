#!/usr/bin/env bash
set -euo pipefail

temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/nswarm-generated.XXXXXX")
cleanup() {
  rm -rf "$temporary_dir"
}
trap cleanup EXIT

./scripts/generate.sh "$temporary_dir"
diff -ru generated/systemd "$temporary_dir"
git diff --exit-code -- generated profiles
