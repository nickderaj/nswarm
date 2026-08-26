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
python3 scripts/run_evals.py
./scripts/check_generated.sh
cargo run --quiet -p fleet -- check .
cargo fmt --all --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo hack check --workspace --feature-powerset --depth 2
cargo deny check
cargo vet check
cargo machete
cargo llvm-cov --fail-under-lines 90 nextest --workspace --all-features

if git cat-file -e origin/main:Cargo.toml 2>/dev/null; then
  require_command cargo-semver-checks
  cargo semver-checks check-release --workspace --baseline-rev origin/main
else
  echo "semver: initial public API baseline will be established by this merge"
fi
