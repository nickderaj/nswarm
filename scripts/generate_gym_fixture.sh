#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <output.sqlite3>" >&2
  exit 2
fi

output=$1
if [[ -e "$output" ]]; then
  echo "refusing to overwrite existing fixture: $output" >&2
  exit 2
fi

sqlite3 "$output" < fixtures/gym/v0-gym-v5.sql
