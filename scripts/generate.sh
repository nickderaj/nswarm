#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/generate.sh <generated-root>" >&2
  exit 2
fi

generated_root=$1
systemd_dir=$generated_root/systemd
tmpfiles_dir=$generated_root/tmpfiles
mkdir -p "$systemd_dir" "$tmpfiles_dir"
cargo run --quiet -p fleet -- render-all . "$systemd_dir" >/dev/null
cargo run --quiet -p fleet -- render-tmpfiles-all . "$tmpfiles_dir" >/dev/null
cargo run --quiet -p fleet -- render-gateway config/hermes-gateway.toml > "$systemd_dir/hermes-gateway.service"
