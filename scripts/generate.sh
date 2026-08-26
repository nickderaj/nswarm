#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/generate.sh <output-directory>" >&2
  exit 2
fi

output_dir=$1
mkdir -p "$output_dir"
cargo run --quiet -p fleet -- render bots/research.toml > "$output_dir/research.service"
cargo run --quiet -p fleet -- render-gateway config/hermes-gateway.toml > "$output_dir/hermes-gateway.service"
