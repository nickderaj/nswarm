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

## Checkpoint C — transactional evidence repositories

Exact implementation SHA: `b45c83cfd98853676e8336e6fdcefecf563179fe`
(`feat(control): enforce evidence repositories`).

Completed acceptance units:

- typed profile and session IDs with transactional profile/session repositories;
- exact job-owned branch namespace/base registration and immutable artifact
  digest recording;
- schema v2 migration from the committed v1 layout adds reviewer attribution and
  is tested from both empty and prior-version databases;
- medium/high-risk candidates cannot become verified without two distinct,
  same-job verifier/reviewer profiles at the exact candidate SHA;
- stale review SHAs, non-review-state submissions, unauthorized reviewer roles,
  job/unit ownership mismatches, unsatisfied dependencies, overlapping paths,
  and multiple topology owners fail closed;
- `just ci` remains green with 38 tests, 9 deterministic hostile-input evals,
  and 90.59% line coverage.

## Checkpoint D — evidence storage boundary

Exact implementation SHA: `cba309a45a4be788060770995089972c4cfc4699`
(`fix(control): redact and validate evidence`).

Completed acceptance units:

- job creation now rejects report contracts that are not object schemas with
  at least one non-empty required field;
- worker reports are checked against the immutable brief's required fields
  before they can enter the evidence ledger;
- every event payload is recursively redacted at the shared persistence
  boundary for secret-bearing field names, private-key markers, and common
  provider/source-control token shapes;
- a negative regression proves incomplete reports fail closed and synthetic
  secrets never reach SQLite while ordinary evidence remains intact;
- `just ci` remains green with 85 files policy-scanned, 39 tests, 9
  deterministic hostile-input evals, and 90.88% line coverage.

## Checkpoint E — manifest-derived host planning

Exact implementation SHA: `98d2b1a220bd046405b3fdfd5dc9b679de28bbcc`
(`feat(fleet): add redacted host planning`).

Completed acceptance units:

- `fleet plan --all` derives its inventory exclusively from `bots/*.toml` and
  rejects duplicate bot names that would collide on installed paths;
- an explicit host root is compared against deterministic systemd and per-bot
  environment artifacts without mutating the host;
- the strict decrypted-secret parser performs no interpolation or ambient
  environment fallback and rejects malformed or duplicate entries;
- environment drift is reported only as clean or redacted replacement, with
  negative tests proving synthetic values never enter plan output;
- unreadable manifest directory entries now fail closed rather than being
  silently omitted;
- `just ci` remains green with 85 files policy-scanned, 43 tests, 9
  deterministic hostile-input evals, and 90.99% line coverage.

Known gaps and external gates:

- GitHub Actions have not run until this branch is pushed and a PR exists.
- Branch protection is pending authenticated owner administration; the current
  local GitHub CLI token is invalid, and no protection settings were changed.
- Merge-tier Miri, Linux sanitizers, mutation, aarch64 execution, and fuzz jobs
  are configured but were not claimed as executed on this macOS host.
- The 38 cargo-vet exemptions are explicit bootstrap debt, not completed source
  audits; new dependencies remain fail-closed.
- `fleet` now validates, inventories, renders, and plans unit plus redacted env
  drift across an explicit host root; host mutation via deploy, status/new
  scaffolding, and direct `age`/`pass` integration remain before the full §4 CLI
  is complete.
- Credential revocation, branch-head updates, richer report-field typing and
  artifact schemas, explicit integrator disposition of non-blocking review
  findings, and profile destruction/audit methods remain before the control
  plane is complete.
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

Then add credential revocation and branch-head update repositories with stale,
unauthorized, and idempotency negative tests. Re-run `just ci` before the next
commit.
