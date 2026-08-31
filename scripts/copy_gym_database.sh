#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 4 ]]; then
  echo "usage: $0 <source-gym.db> <destination-gym.db> [--metadata <metadata.json>]" >&2
  exit 2
fi

exec python3 "$(dirname "$0")/gym_sqlite_copy.py" "$@"
