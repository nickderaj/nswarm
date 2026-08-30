#!/usr/bin/env bash
set -euo pipefail

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required tool $1; see docs/DEVELOPMENT.md" >&2
    exit 2
  fi
}

for command in cargo-nextest cargo-hack cargo-deny cargo-vet cargo-machete cargo-llvm-cov; do
  require_command "$command"
done

python3 scripts/policy_check.py
python3 scripts/validate_profiles.py
./scripts/check_generated.sh
cargo run --quiet -p fleet -- check .
cargo fmt --all --check
cargo check --locked --workspace --all-targets --all-features
python3 scripts/run_evals.py
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo hack check --workspace --feature-powerset --depth 2
cargo deny check
cargo vet check
cargo machete
cargo +nightly-2025-09-18 llvm-cov --branch --fail-under-lines 90 --json \
  --output-path target/coverage.json nextest --workspace --all-features
python3 scripts/check_coverage.py target/coverage.json origin/main

require_command cargo-semver-checks
python3 -m unittest scripts/test_check_semver.py
python3 -m unittest scripts/test_hermes_gateway_spike.py
python3 scripts/check_semver.py origin/main
