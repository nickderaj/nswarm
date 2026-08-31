#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <expected-gym.db> <actual-gym.db>" >&2
  exit 2
fi

exec cargo run --quiet --locked -p gym-bot --bin gym-db-compare -- "$1" "$2"
