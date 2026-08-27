#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "ci-full requires Linux for the repository sanitizer contract" >&2
  exit 2
fi

for command in cargo-mutants cargo-fuzz; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "missing required tool $command; see docs/DEVELOPMENT.md" >&2
    exit 2
  fi
done

cargo +nightly-2025-09-18 miri test -p agent-control policy::
cargo +nightly-2025-09-18 miri test -p agent-control types::
cargo +nightly-2025-09-18 miri test -p botkit
RUSTFLAGS="-Zsanitizer=address" RUSTDOCFLAGS="-Zsanitizer=address" \
  CARGO_TARGET_DIR=target/asan cargo +nightly-2025-09-18 test -Zbuild-std \
  --workspace --all-features --locked --target x86_64-unknown-linux-gnu
RUSTFLAGS="-Zsanitizer=leak" RUSTDOCFLAGS="-Zsanitizer=leak" \
  CARGO_TARGET_DIR=target/lsan cargo +nightly-2025-09-18 test -Zbuild-std \
  --workspace --all-features --locked --target x86_64-unknown-linux-gnu
cargo mutants --workspace
(
  cd fuzz
  cargo +nightly-2025-09-18 fuzz run manifest -- -max_total_time=60
)
