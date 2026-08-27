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

## Checkpoint F — revocable authority and tracked branch heads

Exact implementation SHA: `1edd1f86ef997db8ebcc3387c93f93698e33db56`
(`feat(control): revoke grants and track branch heads`).

Completed acceptance units:

- credential methods are queryable without materializing secrets and only a
  live same-job coordinator profile can revoke an active opaque grant;
- registered branches begin at the immutable base SHA and advance only in
  coding states through compare-and-swap updates;
- a scheduler-tracked branch must match the exact candidate SHA before the
  candidate can enter verification;
- reuse of an event idempotency key with different job/type/payload evidence
  fails closed and rolls back the surrounding state transaction;
- secret redaction now covers verification-verdict and review-finding JSON as
  well as append-only events and worker reports;
- negative tests cover wrong-role revocation, already-revoked grants, stale
  heads, early branch mutation, stale candidate SHA, conflicting idempotency,
  rollback, and secret-bearing verdict/review evidence;
- `just ci` remains green with 85 files policy-scanned, 46 tests, 9
  deterministic hostile-input evals, and 91.86% line coverage.

## Checkpoint G — integrator disposition and profile teardown

Exact implementation SHA: `e52e477d2f74e416dfa71e49f003abd6ecdc5146`
(`feat(control): govern findings and profile teardown`).

Completed acceptance units:

- schema v3 adds durable session destruction and migrates deterministically
  from both the committed v1 shape and schema v2;
- reviewer findings begin unresolved and medium/high-risk candidates cannot
  advance until every exact-SHA finding has a one-shot final disposition;
- only a live same-job integrator can dispose a finding; reviewer
  self-disposition, duplicate disposition, stale findings, unresolved findings,
  and remaining blockers fail closed;
- only a live same-job coordinator can destroy a profile's authority; teardown
  marks its sessions destroyed, releases matching profile leases, prevents new
  sessions/reviews, and emits an append-only audit event;
- filesystem removal is explicitly left to the scheduler after durable
  authority removal rather than being implied by a database flag;
- `just ci` remains green with 85 files policy-scanned, 48 tests, 9
  deterministic hostile-input evals, and 92.75% line coverage.

## Checkpoint H — recursive report and typed artifact contracts

Exact implementation SHA: `86c2aa5ed3aece542c1e491c0381cbfb648410c4`
(`feat(control): type reports and artifact evidence`).

Completed acceptance units:

- immutable briefs now accept only a bounded, recursively validated JSON-Schema
  subset with declared object properties, required fields, arrays/items,
  primitive types, additional-property policy, and a depth limit;
- worker reports are recursively checked for missing, undeclared, and wrong-type
  fields before redacted persistence;
- evidence artifact kinds are an explicit enum and artifact paths must be safe,
  repository-relative locations;
- every new artifact is bound to the unit's exact base/candidate SHA, and stale
  artifact evidence fails closed;
- schema v4 migrates artifact evidence without discarding prior rows and scopes
  uniqueness by source SHA, so identical deterministic output can be valid
  evidence for two distinct revisions;
- malformed schema, nested wrong type, undeclared field, unsafe path, stale SHA,
  prior-schema migration, and same-content/new-SHA regressions pass;
- `just ci` remains green with 85 files policy-scanned, 49 tests, 9
  deterministic hostile-input evals, and 92.82% line coverage.

## Checkpoint I — independent audit and strict CI repair

The overnight implementation was independently reviewed rather than accepted
from its prior local status claim. The severity-ranked findings and supporting
evidence are in [`docs/AUDIT.md`](AUDIT.md). Exact checkpoint SHA:
`72c033092e1698970f2290bc02f647fcea15371a`.

Completed repair units:

- protected-file policy checks no longer trust a local branch name or mutable
  environment variable as authorization; repository symlinks, ignored-test
  syntax variants, integration-test network use, and lint inheritance are
  checked explicitly;
- generated/profile drift checks now include staged, unstaged, and untracked
  files and reject absolute, escaping, or symlinked profile inputs;
- worktree provisioning rejects dangling destination links and symlinked,
  malformed, or special `.git` control paths before invoking Git;
- path leases reject empty, expired, aliased, or escaping resources; an expired
  worker result cannot quarantine a unit after a replacement lease is live;
- merge authorization is bound to a live same-job shipper profile, the exact
  authorized SHA, and the same actor that records the merge;
- evidence redaction covers normalized camel/snake/kebab secret keys, bearer
  credentials, modern provider/source-control token shapes, and private-key
  markers;
- report schemas now require an object root in addition to the bounded recursive
  schema contract;
- PR, merge-group, and nightly workflows use current immutable action SHAs,
  explicit timeouts/concurrency/permissions, selected Miri targets, build-std
  sanitizers, copied-tree mutation, artifacts, MSRV/stable/aarch64 checks, and a
  hosted ARM64 merge job; the unsafe public-repository self-hosted Pi job was
  removed pending an organization-restricted or externally brokered runner;
- coverage now fails unless repository and every crate reach 90% lines, changed
  executable lines reach 95%, and every configured security-critical branch
  outcome is covered.

Verified locally on 2026-08-27:

- `just ci` — pass; 88 files policy-scanned, 2 profiles validated, 9
  deterministic eval checks, 59 nextest tests, doctests/rustdoc, strict Clippy,
  feature powerset, deny/vet/machete, and generated drift checks all pass;
- enforced coverage — 95.64% repository lines, 95.69% changed executable lines,
  100% (54/54) configured critical branch outcomes; per-crate line coverage is
  96.69% agent-control, 99.24% botkit, 90.19% fleet, and 100% research-bot;
- selected pinned-nightly Miri suites pass for `agent-control` policy/types and
  `botkit`; a workspace-wide Miri run is intentionally not claimed because the
  SQLite/filesystem suites are outside the interpreter's supported isolation;
- actionlint v1.7.12 accepts all three workflow files.

## Checkpoint J — GitHub workflow bootstrap and first Linux repair

The one-time default-branch exception is commit
`fe746e4a52f7fdfb803a484f3f63d16ed6d2b5f3`. Its parent is the former `main`
tip, and its complete file list is exactly:

- `.github/workflows/pr.yml`;
- `.github/workflows/merge-queue.yml`;
- `.github/workflows/nightly.yml`.

GitHub now recognizes all three workflows as active. [PR #1](https://github.com/nickderaj/nswarm/pull/1)
targets `main` from `overnight/bootstrap`, with first PR workflow run
[`33077451568`](https://github.com/nickderaj/nswarm/actions/runs/33077451568).
That run proved quality, current stable, MSRV, aarch64 cross-compilation,
reproducibility, and the strict coverage job on GitHub-hosted Linux. Its bounded
fuzz job failed at link time with `undefined symbol: main`: the fuzz manifest
disabled default features on `libfuzzer-sys` without explicitly enabling the
runtime-providing `link_libfuzzer` feature.

This checkpoint enables that exact pinned feature. Locally,
`cargo +nightly-2025-09-18 fuzz build manifest` and the complete `just ci`
contract pass. A macOS fuzz process can build but does not honor the requested
libFuzzer time/run bound on this host, so execution is not claimed locally; the
superseding GitHub Linux run is the authoritative execution check.

## Checkpoint K — exact-head CI and mutation repair

[PR run `33079285064`](https://github.com/nickderaj/nswarm/actions/runs/33079285064)
passed all eight PR jobs at exact head
`d407502f338a0ec9315964b51159736a54686e9c`. The matching
[merge-tier run `33079316077`](https://github.com/nickderaj/nswarm/actions/runs/33079316077)
proved hosted aarch64 execution, the selected Miri suites, AddressSanitizer,
and LeakSanitizer. Its strict mutation job tested 435 mutants and correctly
failed on 60 survivors rather than treating report generation as success.

The repair adds independent negative cases for each compound policy, path,
schema, lease, idempotency, artifact, reviewer, redaction, manifest, gateway,
and CLI operand identified by that report. It also fixes a newly exposed
runtime scope defect: `PathPolicy::can_read` and `can_write` now reject unsafe
lexical paths instead of relying only on brief-time validation. Redundant
event validation and empty-output branches were removed instead of exempted.
The local full run reduced the result to 299 caught, 135 unviable, and two
redundant survivors; after removing those branches, a deterministic iterate
run tested all 129 affected/new mutants with 85 caught, 44 unviable, and zero
missed. `just ci` passes with 66 tests, 96.59% repository lines, 96.28% changed
executable lines, and 100% (56/56) configured critical branch outcomes.

Known gaps and external gates:

- At exact head `47d0f77cc21365f0e215da3efb41ce7b3730494d`,
  [PR run `33116192616`](https://github.com/nickderaj/nswarm/actions/runs/33116192616)
  passed all eight PR jobs and
  [merge-tier run `33116222424`](https://github.com/nickderaj/nswarm/actions/runs/33116222424)
  passed Miri, AddressSanitizer, LeakSanitizer, hosted ARM64, and mutation. A
  follow-up independent review arrived before protection was installed and the
  current repairs will supersede that head; protection remains deferred until
  the repaired exact head passes, avoiding stale required-check evidence.
- GitHub reports no registered self-hosted runner. A personal public repository
  cannot restrict a Pi runner to trusted workflows, so physical Pi execution is
  blocked until an organization runner group or equivalent ephemeral external
  broker can enforce that boundary.
- The 38 cargo-vet exemptions are explicit bootstrap debt, not completed source
  audits; new dependencies remain fail-closed.
- `fleet` now validates, inventories, renders, and plans unit plus redacted env
  drift across an explicit host root; host mutation via deploy, status/new
  scaffolding, and direct `age`/`pass` integration remain before the full §4 CLI
  is complete.
- The control plane's local step-1 repository scope is complete. Physical
  profile-home removal and resource/network enforcement remain scheduler/OS
  adapter responsibilities after the now-audited authority teardown.
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
git push origin overnight/bootstrap
```

Then query the new PR workflow run, inspect every complete job log, and repair
any remaining GitHub-only failure before applying branch protection.

## Checkpoint L — follow-up audit CI and fleet hardening

The independent follow-up findings and their reproduced severity/disposition
are recorded in [`docs/AUDIT.md`](AUDIT.md). This checkpoint repairs the
findings that are composable without the pending control-store schema migration:

- systemd generation derives every bot unit from the manifest inventory rather
  than naming `research.toml`; a two-manifest regression proves complete,
  deterministic rendering;
- bot units add `UMask=0077`, `SystemCallFilter=@system-service`, and systemd
  non-loopback IP denial while retaining loopback Hermes access;
- gateway credentials use an exact model-provider allowlist, secret environment
  values cannot end in a continuation backslash, and deny paths cannot contain
  writable roots as either descendants or ancestors;
- critical coverage is source-marked instead of substring-selected, requires at
  least 30 compiled functions, rejects zero totals and structural mismatches,
  safely aggregates duplicate linked copies, and reports each uncovered branch
  with source coordinates;
- new negative tests retain all 34 critical functions and close every newly
  exposed outcome rather than shrinking the coverage set.

Fresh local evidence before checkpointing: 71 workspace tests pass; strict
Clippy passes; policy scans 88 files; both profiles validate; the current nine
eval checks pass; generated output matches the temporary regeneration (the
pre-commit cleanliness guard correctly reports the intentional checked-in
service update); repository line coverage is 97.11%, changed executable-line
coverage is 97.07%, every crate exceeds 90%, and critical branch coverage is
100% (232/232 across 34 marked functions).

The active blockers before protection remain the current exact-head GitHub
runs and the unsatisfiable CODEOWNER-review requirement on a repository whose
only collaborator is the PR author. The multi-unit/dependency schema,
attributed verdict authority, Git subprocess isolation, recovery transitions,
capability-backed store authorization, job-scoped topology leases, and real
Rust-driven eval corpus remain active audit findings; none is claimed repaired
by this checkpoint.
