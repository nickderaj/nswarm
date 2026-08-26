# nswarm v1 build status

Updated: 2026-08-27 (Europe/London). Active branch: `overnight/bootstrap`.
The frozen `ultron` v0 checkout has not been modified, staged, committed,
deployed, restarted, or used as a source of private state.

## Checkpoint A — step-1 foundation candidate

Exact implementation SHA: `1fae9da69dbcdb4a48804354954c89a8a9e184c0`
(`feat: establish v1 control-plane foundation`).

Completed acceptance units:

- pinned Rust 1.90.0 toolchain, explicit MSRV, exact dependencies, and committed
  root and fuzz lockfiles;
- strict workspace lint contract forbidding unsafe code and denying warnings,
  Clippy all/pedantic/nursery/cargo, formatting, compile, test, doctest, rustdoc,
  feature-powerset, dependency, vet, unused-dependency, generated-file, policy,
  profile, eval, and 90% line-coverage gates under `just ci`;
- separate pinned-action PR, merge-queue, and nightly workflows for Linux,
  aarch64, Miri, address/leak sanitizers, mutation, and bounded fuzzing;
- deterministic `botkit` D5 contracts: plain-text conversations, generic
  `(surface, external_id)` update keys, attributed untrusted context, and a
  streaming provider boundary;
- typed fleet manifest and deterministic bot/gateway systemd rendering with a
  mandatory unprivileged user, per-bot env file, sandbox baseline, loopback
  gateway, state/data-only writable roots, and star-topology peer validation;
- immutable machine-validated job briefs, explicit roles/capabilities and job
  states, transactional SQLite tables for every §7.5 entity, append-only events,
  idempotency, expiring/overlap-safe leases, exact-SHA verdict invalidation,
  integration reverification, and merge authorization;
- safe local worktree provisioner plus no-secret credential broker;
- root-owned research and coder SOUL/skill bundles, canonical generated profile
  checks, hostile-input fixtures, deterministic containment/evidence evals, and
  pinned pstack provenance;
- CODEOWNERS, branch-protection documentation and a reproducible GitHub setup
  script that has not been applied automatically.

Verified commands and outcomes:

- `just ci` — pass; 81 files policy-scanned, 2 profiles validated, 9 model-free
  eval checks, 33 nextest tests, doctests and rustdoc pass;
- `cargo llvm-cov --fail-under-lines 90 nextest --workspace --all-features` —
  pass at 90.72% repository line coverage;
- `cargo deny check` — advisories, bans, licences, and sources pass;
- `cargo vet check` — pass with 38 explicit initial exemptions;
- `cargo machete` — no unused dependencies;
- `cargo hack check --workspace --feature-powerset --depth 2` — pass;
- generated systemd/profile byte comparison and clean-diff check — pass.

## Checkpoint B — manifest-derived fleet drift gate

Exact implementation SHA: `648f1397155dd204f4e206c2ff2d8d688df052ad`
(`feat(fleet): add drift and inventory checks`).

Completed acceptance units:

- `fleet check` discovers every `bots/*.toml` file without a maintained list
  and verifies its profile sources and crate/package scaffold exist;
- `fleet render <manifest> --diff <installed-unit>` returns `clean` only for a
  byte-identical installed artifact and exposes complete drift otherwise;
- the research manifest now points to an explicit fail-closed workspace
  scaffold rather than a nonexistent executable;
- `just ci` now runs the repository inventory check and remains green with 36
  tests and 90.57% line coverage.

Known gaps and external gates:

- GitHub Actions have not run until this branch is pushed and a PR exists.
- Branch protection is pending authenticated owner administration; the current
  local GitHub CLI token is invalid, and no protection settings were changed.
- Merge-tier Miri, Linux sanitizers, mutation, aarch64 execution, and fuzz jobs
  are configured but were not claimed as executed on this macOS host.
- The 38 cargo-vet exemptions are explicit bootstrap debt, not completed source
  audits; new dependencies remain fail-closed.
- `fleet` now validates, inventories, renders, and diffs local installed-unit
  fixtures; host mutation via deploy, status/new scaffolding, and encrypted
  secret materialization remain before step 1 is complete.
- Agent store tables exist for profiles, sessions, branches, artifacts, review
  findings, and credential grants; typed repository methods for the remaining
  records and negative tests remain before the control plane is complete.
- The checked-in research service manifest is a rendering/policy fixture; no
  writable research agent or bot behaviour is enabled.
- Crate publication was not attempted. The remote repository already supplies
  an MIT licensing decision, but crates.io credentials/ownership were not
  available and reserving a name with a placeholder crate is prohibited.
- Hermes is pinned only for the future step-3 harness. No gateway behaviour,
  warm-agent reuse, multiplex isolation, prompt size, credentials, systemd
  sandbox, or socket ACL measurement is claimed.

Next executable action after checkpointing:

```console
cargo test -p agent-control
```

Then add typed profile/session/branch/artifact/finding repositories and their
unauthorized/stale negative tests, followed by `fleet render --diff` against a
temporary host-root fixture. Re-run `just ci` before the next commit.
