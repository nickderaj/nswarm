# Development and quality gates

The workspace MSRV and pinned default toolchain are Rust 1.90.0. Dependency
versions and quality-tool versions are exact so a tool update is a visible
policy change.

Install the PR-tier tools:

```console
cargo install just --version 1.58.0 --locked
cargo install cargo-nextest --version 0.9.128 --locked
cargo install cargo-hack --version 0.6.45 --locked
cargo install cargo-deny --version 0.20.2 --locked
cargo install cargo-vet --version 0.10.2 --locked
cargo install cargo-machete --version 0.9.2 --locked
cargo install cargo-semver-checks --version 0.46.0 --locked
cargo install cargo-llvm-cov --version 0.9.0 --locked
```

Then run `just ci`. Ordinary tests are hermetic: no live Telegram or Hermes
service, model provider, public network, host state, private gym database, or
real credential is required. The gym MCP integration uses only a temporary
local Unix socket and a sanitized committed SQLite fixture.

`just ci-full` additionally requires Linux, `nightly-2025-09-18` with Miri,
`cargo-mutants` 27.1.0, and `cargo-fuzz`. Sanitizer and Miri commands are pinned
in `scripts/ci-full.sh`; the merge queue runs them after the PR gate.

## Lint exceptions

Workspace-wide and crate-wide allowances are prohibited. The narrowest
item-level `allow` or `expect` may be used only with Rust's
`reason = "..."` syntax. The repository policy checker rejects an unreasoned
suppression.

## Generated artifacts

`just generate` renders systemd fixtures. `just ci` regenerates into a temporary
directory, compares bytes, and also fails if tracked generated/profile files are
dirty. Generated output is never accepted automatically by CI.
