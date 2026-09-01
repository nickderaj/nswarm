# nswarm v1 build status

## Checkpoint AC - D23 live provider-cache gate

Updated: 2026-09-01. The cost-bounded live trial combined the pinned Hermes
prompt-restoration/cache-marker path with four matched cold-versus-long-lived
provider requests. The long-lived prime wrote 6,435 cache tokens; each of three
repeats read all 6,435 while matched cold sessions wrote the prefix again.
Repeated-turn modeled cost fell from 24,391 to 2,190 micro-USD (91.02%), and
median end-to-end latency was 9.602 seconds long-lived versus 32.486 seconds
cold.

D23 is revised to retain the stable HTTP session route despite fresh local
`AIAgent` construction. D24 is unblocked and remains unevaluated; multiplexing,
credential/profile/tool/socket isolation, the dedicated-user sandbox, Pi
RSS/latency and prompt size still decide between one gateway and the documented
per-bot-process fallback. No D24 run, gym parallel trial, bot session,
deployment, live pilot or coder-concurrency increase occurred.

The harness required explicit operator opt-in and a $1 hard ceiling; its
conservative full-run reservation was $0.2884 and the usage-derived total was
42,776 micro-USD. The credential and raw provider material were not persisted.
The aggregate evidence and fail-closed derivation check are in
`spikes/hermes/evidence/provider-cache.json` and
`scripts/check_hermes_provider_cache_evidence.py`.

## Checkpoint AB - D23 native-adapter architecture gate

Updated: 2026-08-31. The pinned Hermes native messaging path reuses cached
agents, but exposes no stable external request-response contract that also
preserves `botkit` ownership of Telegram and synchronous `ask()` semantics. A
custom adapter would bind nswarm to private Python ingress and lifecycle APIs.
Hermes Relay is closer to the required split, but the pinned adapter, transport
and frame protocol are explicitly experimental and require a separate connector
contract outside the reviewed pin.

The source verifier now pins those relevant files and emits the three-candidate
decision in `spikes/hermes/evidence/native-adapter.json`. No D23 replacement,
Hermes session, model call, D24 measurement, fallback topology, live pilot,
deployment or concurrency increase is claimed. The stable HTTP transport remains
a candidate; provider-side byte-prefix cache behavior and cost on repeated turns
are the next D23 measurement. A deterministic fake cannot establish that result.
Coder concurrency remains one.

## Checkpoint AA - Step 5 research and serial-coder boundary

Updated: 2026-08-31. Active branch:
`codex/step5-research-coder-pilot`, based directly on `origin/main`
`f4a40e07e226757f218c0aa45d992237c43a21e0`. The implemented and deferred
boundary is recorded in [`STEP5_STATUS.md`](STEP5_STATUS.md).

The branch implements schema-closed research evidence with a reserved critic
attestation field, exact-SHA coder handoffs bound to live path leases, transactional
typed persistence, replay-safe provisioning, fixed independent control roles,
and a deterministic adapter that rejects a second sequential coder. Full `just ci` passes with
253 files policy-scanned, 3 profiles validated, 8 production-backed eval cases,
211 nextest tests, doctests, rustdoc, dependency checks, and 196 semver checks
per crate. Strict coverage passes at 1,305/1,327 changed executable lines
(98.34%), 318/318 critical branch outcomes (100.00%), and 10,267/10,576
repository lines (97.08%).

D23 is now settled by the later provider-cache evidence even though the pinned
Hermes HTTP route creates a fresh `AIAgent` per request. D24 remains unevaluated.
Therefore no live Hermes research or coder pilot, ask surface, fallback
topology, or concurrency above one is claimed. Deterministic startup and
containment tests do not satisfy the live serial-pilot gate. This PR was
produced through the ordinary maintainer workflow, and coder concurrency
remains one.

The deterministic adapter does not yet prove atomic multi-process admission or
rollback after partial startup. Those runtime properties remain gated with the
live pilot after D24 settles.

The frozen `ultron` checkout remained read-only. No credentials, private
databases, transcripts, production state, or private paths were accessed.

## Checkpoint Z — Step 4 gym parallel-run candidate

PR #5 is based directly on merged PR #4
`b150ced3e77bfc83d47d3e75bb11864eebe0e72b`. The implementation and remaining
external gates are recorded in [`gym/STEP4_STATUS.md`](gym/STEP4_STATUS.md).
The v0 checkout remains read-only and no production credential or private gym
database has been accessed. D23 is settled by the later provider-cache result,
but D24 remains unresolved, so conversation behavior is explicitly unavailable
rather than built against an unevaluated topology.

Updated: 2026-08-30 (Europe/London). Active branch:
`codex/post-step3-followups`, based directly on merged PR #3 main
`c8c1b69390f607be7f0ae895441b2141c74a3d93`.
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

Fresh local evidence before checkpointing: 72 workspace tests pass; strict
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

## Checkpoint M — subprocess and capability boundary repair

Follow-up audit repairs now isolate Git worktree creation from ambient process
and user configuration, disable hooks, fsmonitor, credential helpers, prompts,
and file transport, and prove a hostile repository checkout hook cannot run.
Existing actor-bearing control-store operations authorize through the typed
role-to-capability map rather than duplicated SQL role literals. The structured
profile vocabulary now exactly matches the Rust capability enum. Topology
leases conflict within a job while independent jobs may integrate concurrently.
The actionlint Go module is pinned to the full official `v1.7.12` commit, and
repository policy enforces immutable Go tool revisions.

Focused local evidence: all 48 `agent-control` tests pass, including the new
hook and independent-topology regressions; strict workspace Clippy, profile
validation, policy validation, and immutable-commit actionlint pass. Fresh
workspace coverage runs 73 tests and reports 97.08% repository lines, 97.23%
changed executable lines, every crate above 90%, and 100% (232/232) branch
outcomes across 35 marked critical functions.

This checkpoint does not claim that every lifecycle method has actor identity.
Attributed exact-SHA verification remains the next critical control-store
repair, alongside the multi-unit job/dependency migration.

## Checkpoint N — attributed exact-SHA verification

Schema v5 adds verifier attribution to persisted verification verdicts and
uniqueness per unit, exact SHA, and verifier profile. The typed verdict request
requires a live same-job `Verify` capability before persistence, records the
actor in the redacted event ledger, and makes any attributed failing verdict
block that exact SHA. Nullable verdicts migrated from older schemas cannot
authorize acceptance, so historical anonymous evidence fails closed and must
be rerun.

Negative tests prove an unknown profile and a live coder cannot publish a
verdict, a dissenting attributed failure cannot be overridden by a pass, and a
legacy anonymous pass cannot authorize the SHA. Schema migration remains
idempotent from the committed v1 layout and exposes the new attribution column.
Strict Clippy and all 50 `agent-control` tests pass. Fresh workspace coverage
runs 76 tests and reports 97.14% repository lines, 97.20% changed executable
lines, every crate above 90%, and 100% (238/238) branch outcomes across 35
marked critical functions.

The matching merge tier for prior head
`e3433b60958de6912f70dd16f270dc56bcf177fb` passed in
[run `33118132883`](https://github.com/nickderaj/nswarm/actions/runs/33118132883).
GitHub did not create a pull-request suite for that revision, so it is not used
as protection evidence; the next push must produce and pass a new PR suite
before protection can be applied.

## Checkpoint O — multi-unit schema and dependency integrity

Schema v6 separates immutable per-unit briefs from job identity. A job now pins
its repository and standing-policy version once while owning multiple units;
each unit retains its own report contract. Dependencies must already exist in
the same job, both dependency columns are foreign keys, and dependent leasing
remains blocked until every prerequisite is `merged`. Credential identifiers
remain job-scoped and cannot change their allowed methods between unit briefs.

Creation uses one immediate transaction across the optional job row, unit,
unit brief, dependencies, credential grants, and append-only event. Negative
tests prove unknown dependencies, cross-job dependencies, repository or policy
changes, and credential-method conflicts roll back without partial rows.
Migration tests populate each historical schema version from v1 through v5,
upgrade it to v6 twice, preserve the brief, verify both dependency foreign keys,
confirm `PRAGMA foreign_keys = 1`, and require an empty
`pragma_foreign_key_check` result. Direct SQL tests also prove job scope and
unit briefs are immutable.

Fresh pre-checkpoint evidence: all 54 `agent-control` tests pass, strict
workspace Clippy is clean, and the complete local `just ci` gate passes. Its
fresh coverage run executes all 79 workspace tests and reports 98.06% for
`agent-control`, 97.31% repository line coverage, 97.25% changed executable-line
coverage, every crate above 90%, and 100% critical branch outcomes (252/252
across 36 marked functions). Exact-head GitHub PR and merge-tier runs remain
pending for the checkpoint commit and are not yet claimed as evidence.

The committed checkpoint is
`f23e56f15edbf7631e5b2e2f83e5827509d6ff2a`. All eight PR jobs passed in
[run `33150146844`](https://github.com/nickderaj/nswarm/actions/runs/33150146844),
including 79 tests, 97.31% repository coverage, 97.44% PR changed-line
coverage, and 252/252 critical outcomes. All four exact-head merge-tier jobs
passed in
[run `33150797402`](https://github.com/nickderaj/nswarm/actions/runs/33150797402):
464 mutation candidates produced no survivors (320 caught, 144 unviable),
Miri passed, hosted aarch64 passed, and ASan/LSan passed.

## Checkpoint P — recoverable exact-SHA authorization and command replay

Schema v7 replaces the one-row-per-unit merge authorization with immutable
history plus an explicit invalidation timestamp and one-active-per-unit index.
`Integrated` and `MergeAuthorized` may recover only through a dedicated
actor-bearing command: an integrator may recover the former and a shipper may
recover the latter. Recovering an authorized merge invalidates its active
authorization, while a replacement candidate must traverse fresh exact-SHA
verification, integration, and authorization. Generic state transitions are
unable to bypass the recovery gate.

An immutable command-result ledger now makes advertised idempotent commands
replay before state preconditions. Identical retries return their original
result after later state changes; conflicting command/request reuse fails; and
the command row is transactionally linked one-to-one with its append-only event.
Tests cover transition, candidate, verdict, acceptance, integration, merge,
recovery, report, teardown, branch, and review-disposition replay paths.
Populated schema-v6 migration preserves its existing authorization row, retains
foreign-key integrity, and remains idempotent.

Fresh local evidence: all 56 `agent-control` tests and strict workspace Clippy
pass. The complete `just ci` gate executes 81 tests with none skipped, keeps all
38 supply-chain exemptions explicit, and reports 97.50% `agent-control`, 96.94%
repository, and 96.62% changed executable-line coverage. Critical branch
coverage is 278/278 across 37 marked functions. Exact-head GitHub PR and
merge-tier evidence remain pending for this checkpoint commit.

The committed checkpoint is
`5494e50deb921942e548f87dcff0b33c7ec16df6`. All eight PR jobs passed in
[run `33152626713`](https://github.com/nickderaj/nswarm/actions/runs/33152626713),
including 81 tests, 96.94% repository coverage, 97.00% PR changed-line
coverage, and 278/278 critical outcomes. The exact-head
[merge-tier run `33181311813`](https://github.com/nickderaj/nswarm/actions/runs/33181311813)
passed hosted aarch64, Miri, AddressSanitizer, and LeakSanitizer, but correctly
failed the checkpoint because mutation testing found two survivors: the
independent operands of the integration-recovery gate and the command identity
blank-field gate. Mutation tested 481 candidates: 331 caught, 148 unviable,
and 2 missed. This run is recorded as failed evidence and is not used to claim
the checkpoint merge-ready; both missing outcomes are covered by checkpoint Q.

## Checkpoint Q — actor-bound mutation and lease authority

Schema v8 adds an immutable `holder_profile` foreign key to leases and a
storage trigger requiring every new holder to be live in the lease's exact job
and unit. Historical holderless leases migrate without invented attribution
and fail closed. Lease acquisition now requires a live same-job coordinator
and a live exact-unit holder with capability appropriate to the lease kind;
profile lease resources must equal the holder identity.

The nine reported actorless mutations now carry and persist actor identity.
State transitions, candidate/branch mutation, integration completion,
worker-result acceptance, report publication, and artifact recording enforce
the exact role capability and required actor-owned lease in the same immediate
transaction. Verdict acceptance and the previously actor-bearing verdict,
review, merge, and recovery paths were also tightened to exact-unit scope and
leases where their roles support them. A worker result cannot substitute a
path or topology lease for its actor-owned profile lease. Profile destruction
releases every lease held by that profile, and integration recovery releases
stale topology ownership before replacement work. Provisioning and raw event
storage methods are crate-private until the scheduler command adapter exists,
so they are not downstream mutation bypasses.

Adversarial tests cover unauthorized coordinators, wrong-role holders,
cross-job and cross-unit actors, destroyed profiles, expired leases, wrong
lease holders and kinds, missing profile/topology leases, and merge completion
by a live shipper other than the exact authorized actor. Populated v1 through
v7 schemas migrate twice to v8 with active, clean foreign keys; a legacy
holderless lease remains unattributed. Regression tests also kill both mutants
missed by merge-tier run `33181311813`: a recovery call cannot synthesize the
otherwise legal `Integrated` to `MergeAuthorized` edge, and each blank command
identity operand fails independently.

Fresh pre-checkpoint local evidence: strict `agent-control` Clippy passes; all
60 `agent-control` tests pass; the complete `just ci` gate executes 85
workspace tests with none skipped, validates 88 policy-scanned files and two
profiles, keeps all 38 cargo-vet exemptions explicit, and reports 97.70%
`agent-control`, 97.17% repository, and 96.76% changed executable-line
coverage. Critical branch coverage is 288/288 across 37 marked functions.
Exact-head GitHub PR and merge-tier evidence remain pending for checkpoint Q.

The committed checkpoint is
`b128fa8dfd9aa1aba99a02f957b8b4d21d9b481b`. All eight PR jobs passed in
[run `33183866332`](https://github.com/nickderaj/nswarm/actions/runs/33183866332),
including 85/85 tests with none skipped, all policy/profile/eval/generated
gates, deny/vet/machete, reproducibility, bounded fuzzing, MSRV, current stable,
hosted aarch64 compilation, 97.17% repository coverage, 97.20% PR
changed-line coverage, and 288/288 critical branch outcomes. The complete
17,427-line log contains no Actions error or warning annotations. All four
exact-head merge-tier jobs then passed in
[run `33208168688`](https://github.com/nickderaj/nswarm/actions/runs/33208168688):
hosted aarch64, Miri, AddressSanitizer/LeakSanitizer, and mutation testing are
green. Mutation tested 491 candidates with no survivors (339 caught and 152
unviable); the complete 1,833-line log has no Actions error or warning
annotations. Checkpoint Q is therefore complete at its exact committed SHA.

## Checkpoint R — production-backed evaluation corpus

The eval gate no longer makes simplified Python copies of Rust security
decisions. Five committed schema-v1 cases supply adversarial inputs and expected
decisions for role capabilities, exact-SHA verification binding, repository
path containment, recursive evidence redaction, and lifecycle transition
policy. Each case names one Rust unit test that parses the same committed JSON
and exercises production `Role`, `Capability`, `PathPolicy`, `JobState`, `Sha`,
redaction, and transactional store behavior.

The Python runner enforces the exact case schema, file/id agreement, unique
case and test identities, and the required security-case set. It invokes Cargo
without a shell using `--locked --offline`, caps execution time, prints Rust
failure output, and verifies that every exact named test emitted an `ok` result.
It contains no competing redaction, capability, SHA, path, or transition
implementation and cannot pass merely because Cargo exited successfully after
filtering out an intended test. Synthetic token fragments are assembled only
inside the Rust test so the committed corpus does not itself resemble a usable
credential.

Fresh pre-checkpoint local evidence: the corpus runner reports all five named
Rust tests, strict Clippy passes, and the complete `just ci` gate executes 87
workspace tests with none skipped. Policy scans 89 files, both profiles and the
manifest validate, all 38 cargo-vet exemptions remain explicit, and coverage is
97.66% for `agent-control`, 97.15% repository-wide, and 97.11% across changed
executable lines. All 288 critical branch outcomes across 37 marked functions
remain covered. Exact-head GitHub PR and merge-tier evidence remain pending for
checkpoint R.

The first exact-head PR attempt for `2c229a6e4c680a9c6ad6a1a197e706c0aa1c983e`
is retained as failed evidence in
[run `33208327933`](https://github.com/nickderaj/nswarm/actions/runs/33208327933).
The quality job reached the offline eval runner before any workspace dependency
command had populated a clean runner's Cargo cache, so Cargo correctly refused
to fetch `rusqlite` in offline mode. The PR and local CI sequences now perform a
locked workspace check before the offline eval step. This prepares the locked
dependency graph in the ordinary build phase while keeping the eval runner
itself network-disabled; a later exact-head run must pass before checkpoint R
is claimed as GitHub evidence.

## Checkpoint S — explicit Telegram and socket-group rendering

`Surface::telegram` now controls the rendered network policy. A disabled
adapter retains systemd's deny-all IP policy and loopback exception for the
local Hermes gateway. An enabled adapter receives neither directive: this
avoids making Telegram impossible without claiming that systemd can reliably
contain a changing hostname. Restricting Telegram-enabled egress remains a
separate, unimplemented firewall or network-namespace adapter.

Every bot manifest now declares its dedicated `<bot>-access` group. Validation
derives the only accepted value from the bot name, preventing a manifest from
adding the service to `wheel`, another bot identity, or another pre-existing
host group. The service renders that identity through `SupplementaryGroups` so
a future socket ACL adapter has an explicit group contract. This checkpoint
does not claim that the group exists, that peers have membership, that the
runtime assigns socket ownership or mode, or that a real systemd host enforces
the text.

Focused evidence: all 21 fleet library/binary tests pass, including explicit
Telegram-enabled, non-Telegram, dedicated-group, repository-generation, and
CLI rendering cases. Strict fleet Clippy passes, and a fresh temporary
generation is byte-identical to the checked-in systemd tree. After checkpointing,
the full clean `just ci` gate passes 89 workspace tests with none skipped,
validates all policy/eval/generated and supply-chain gates, and reports 97.17%
repository and changed executable-line coverage with 290/290 critical branch
outcomes. Exact-head GitHub evidence remains pending.

## Checkpoint T — immutable fail-closed verifier results

The one-verdict-per-unit/SHA/verifier policy is intentional. Schema v9 adds
update and delete guards to the existing uniqueness constraint, so an
attributed failure cannot be erased, revised, or shadowed by a later pass from
the same verifier. Acceptance already aggregates every attributed verdict and
therefore continues to reject an exact SHA when any verifier failed it.

Recovery is deliberately content-addressed rather than verdict-mutable: the
unit enters `fix-required`, the author records a new candidate SHA, and that
new SHA receives fresh verification. The end-to-end regression proves a
failed SHA remains unacceptable, direct insert/update/delete rehabilitation is
rejected by SQLite, and the same verifier can pass a new SHA which then
advances. Populated v1 through v8 databases migrate idempotently to v9. This
policy prefers a small corrective commit over weakening durable negative
evidence; operational false positives therefore cost a new SHA by design.

## Checkpoint U — supply-chain exemption inventory

The locked graph still contains exactly 38 cargo-vet exemptions and zero local
audits. The generated [inventory](SUPPLY_CHAIN.md) now records every crate's
exact version and crates.io source, exact-release publishing account when the
registry exposes one, dependency kind, a representative shortest use path,
required criterion, and review class. The graph divides into 6
direct/security-sensitive, 18 transitive bootstrap, and 14 build/dev-only
entries.

No exemption was promoted without source-review evidence. P1 direct and native
SQLite boundary review is assigned first, P2 transitive bootstrap review or
trustworthy audit imports second, and P3 build/dev-only review third. Build
tooling remains relevant to produced artifacts even when it does not execute in
the service. `cargo vet check` remains in `just ci`, so newly resolved versions
continue to fail closed. `scripts/inventory_vet.py` regenerates the table from
the vet configuration, Cargo's graph, and crates.io exact-version records.

## Checkpoint V — final exact-head evidence and protected main

The repaired content head is
`8ce78f1f2bf543a45b27c89d72b131ba7bac5849`. A brand-new target directory
(`/private/tmp/nswarm-clean-target.9rHl8b`) passed the complete `just ci` gate,
followed by the ordinary repository target passing the same gate. Both runs
executed 90/90 tests with none skipped, retained all policy, generated,
supply-chain, documentation, feature-power-set, and coverage checks, and
reported 97.18% repository and changed executable-line coverage with 292/292
critical branch outcomes.

All eight jobs passed at that exact SHA in
[PR run `33209535355`](https://github.com/nickderaj/nswarm/actions/runs/33209535355).
The complete 16,942-line log contains no Actions error/warning annotations or
failure markers. It records 90/90 tests, five production-backed evals, 91
policy-scanned files, two profiles, 38 explicit cargo-vet exemptions, clean
deny/vet/machete results, reproducible release outputs, MSRV 1.90, current
stable, aarch64 compilation, 1,688,062 bounded fuzz executions, 97.18%
repository and changed-line coverage, and 292/292 critical branch outcomes.

All four jobs then passed against the same exact SHA in
[merge-tier run `33210099911`](https://github.com/nickderaj/nswarm/actions/runs/33210099911).
The complete 1,849-line log contains no Actions error/warning annotations or
failure markers. Hosted aarch64 ran all 90 tests, Miri and Address/Leak
Sanitizer passed, and mutation tested 494 candidates with 342 caught, 152
unviable, and zero survivors.

After those results, `main` protection was applied and queried back through the
GitHub API. A subsequent exact check-runs query established that GitHub reports
the bare job names below; the Actions UI's composite labels (for example,
`PR / quality`) are display labels rather than status contexts. The corrected
material returned configuration is:

```json
{
  "required_status_checks": {
    "strict": true,
    "checks": [
      {"context":"quality","app_id":15368},
      {"context":"dependencies","app_id":15368},
      {"context":"coverage","app_id":15368},
      {"context":"msrv","app_id":15368},
      {"context":"current-stable","app_id":15368},
      {"context":"aarch64-compile","app_id":15368},
      {"context":"bounded-fuzz","app_id":15368},
      {"context":"reproducibility","app_id":15368},
      {"context":"sanitizers","app_id":15368},
      {"context":"miri","app_id":15368},
      {"context":"mutation","app_id":15368},
      {"context":"aarch64-hosted","app_id":15368}
    ]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "require_last_push_approval": true,
    "required_approving_review_count": 1
  },
  "required_linear_history": true,
  "required_conversation_resolution": true,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "block_creations": false,
  "allow_fork_syncing": false,
  "restrictions": null
}
```

That was the final pre-merge state of the bootstrap branch. PR #1 was
subsequently approved and squash-merged as
`ac4bfd5f35a1aa2fbbf76ed46f84e7644ca7b049`; protected `main` is the direct
base for Step 2. No Step 2 merge has been performed.

## Checkpoint W — Step 2 gym vertical-slice candidate

PR #2 begins the smallest genuine Step 2 slice. The new `gym-bot` crate keeps
teloxide types at a recorded-update adapter edge and accepts neutral actor,
`(surface, external_id)`, and text input in its command core. `/weight <kg>`
uses an injected clock, checks the configured owner before idempotency, rejects
blank, malformed, non-finite, zero, and negative values, writes the frozen v0
`body_metrics` intent, and matches Python's six-significant-digit success
format. A separate SQLite sidecar persists generic update identities across
service restarts without changing the v0 gym schema.

The production MCP handler exposes exactly one bounded, read-only
`body_metrics` tool. An actual rmcp client and server negotiate and exchange
requests over a temporary Unix socket; contract tests reject malformed frames,
unknown tools, invalid bounds, and unavailable storage, verify the tools-only
capability map, prove socket cleanup, refuse a public runtime directory, and
enforce an owner-only `0700` parent plus socket mode `0600` on every supported
Unix platform. No filesystem, shell, raw SQL,
arbitrary network, resource, prompt, sampling, or Hermes surface is exposed.

The parity corpus contains a schema-v1 fixed-time `log_body_weight` intent, an
empty sanitized schema-v5 SQLite fixture reconstructed from frozen v0 commit
`2d7052011c17bd028fdae0fdfd521918c11de560`, and a golden row captured by that
commit's real `ActivityRepository.log_metric`. CI layers the committed golden
row onto the empty schema; it does not regenerate expected state from v1 SQL.
The candidate converts the fixed instant through the production configured-zone
clock. Deterministic snapshots compare
`user_version`, all 15 application tables, columns, exact DDL, eight indexes,
all losslessly normalized row values, and foreign-key violations with an empty
nondeterminism allow-list. Deliberate tests detect missing and extra rows,
unexpected tables, column/index/schema-version/value drift, and foreign-key
failures, including a deliberate UTC-text regression. The parity gate needs
neither the sibling v0 checkout nor its Python environment.

Fresh local evidence against code checkpoint `201261d`: all 38 gym tests and
131 workspace tests pass, including real Unix-socket protocol tests on macOS,
four property tests, recorded Telegram-shaped inputs, startup/schema checks,
positive parity, and deliberate mismatch cases. The complete `just ci` gate
passes both from the ordinary target and a brand-new isolated target, covering
strict Clippy and rustdoc, generated and policy checks, feature powerset,
deny/vet/machete, coverage, and semver checks. Coverage is 94.44% for `gym-bot`,
97.02% repository-wide, 95.55% across changed executable lines, and 296/296
configured critical branch outcomes. Focused mutation tests process all 126 gym
mutants with 63 caught, 63 unviable, and zero survivors or timeouts.

The expanded dependency graph passes all four Cargo Deny categories. Cargo Vet
reports 46 fully audited packages, 12 partially audited packages, and 156
fully exempted packages using the configured imported audit registries. The Vet
configuration grew by 130 exemption records, from 38 to 168; the additional 12
records are audit-chain bases that Vet reports as partially audited. This is
explicit bootstrap trust debt, not 130 completed source audits. The
repository-local Aquamarine shim displaces an abandoned docs-only macro chain;
its graph-wide and non-propagating publication limitations and upstream removal
trigger are documented beside the shim.

Recorded non-blocking post-spike follow-ups:

- choose and test an explicit policy for legacy or corrupt non-RFC-3339 stored
  timestamps; the current fail-closed whole-call error must not be replaced by
  silent row loss without a warning or partial-result contract;
- revisit the mutation-driven `OpenFlags::union` and string `.eq()` spellings
  only if they can be made clearer without broad function-level mutation skips
  that would suppress meaningful mutants;
- measure the resolved graph and evaluate consolidating direct Chrono call
  sites onto Jiff in a separate timestamp-focused change; do not claim an
  exemption reduction until transitive Chrono features are shown to disappear.

This checkpoint does not prove live Telegram polling or delivery, Raspberry Pi
execution, Fleet-assigned socket group ownership, systemd behavior, Hermes
compatibility, private-database operation, or cutover readiness. The Step 2
parser deliberately omits bare `/weight` history and rejects trailing tokens
and Python underscore numerics. MCP accept retry, connection caps, handshake
timeouts, and stale-socket recovery remain pre-deployment work. Exact-final-SHA
GitHub jobs and independent re-review remain pending.

## Checkpoint X — Step 3 Hermes HTTP architecture gate

PR #2 was verified merged into protected `origin/main` at
`5d3f7ef4cb449df3cd9a90d4742a651140c6f3d9` before the Step 3 branch was
created. The frozen v0 checkout remained clean and read-only.

The reviewed Hermes pin is `v2026.8.19`: annotated tag object
`b05e680e63d39d5a8e3ec0f5842a41d1c4209c03`, peeled source commit
`fcbd1076a93841fa88855acce810e342a5b78101`, package `hermes-agent==0.20.5`,
Python `>=3.11,<3.14`. Relevant source and lockfile bytes are fail-closed
SHA-256 pins.

The first Step 3 gate failed. The authenticated
`POST /api/sessions/{id}/chat` handler calls `_run_agent`, which constructs a
fresh `AIAgent` unconditionally for every request. The API adapter has no agent
cache; only the native messaging path uses the gateway's LRU. MCP discovery is
process-wide, and the exact system prompt can be restored from SQLite after its
first construction, but each request still rebuilds the agent object, memory,
tool snapshot and in-memory state.

The deterministic route harness used the pinned Python 3.12.12 environment on
Darwin arm64, an ephemeral loopback listener, temporary state, explicit session
IDs, real aiohttp/SessionDB/executor code and a fake local provider boundary.
Thirty new-session samples measured 0.347/0.390/0.411/0.584/0.654 ms
(min/median/mean/p95/max). Thirty repeated-session samples measured
0.338/0.359/0.372/0.433/0.446 ms. The separate global route prime was
90.796 ms. All 62 chat calls invoked the factory; the 31 repeated-session calls
created 31 distinct agent instances. These numbers are route overhead, not
live-provider, real-agent-construction, growing-transcript, provider-cache or
Pi latency. The new/repeated classes exercise the same fake-agent path and are
not represented as actual cold/warm agent-construction measurements.

The full authenticated `/v1/capabilities` response and its canonical hash are
committed with the raw samples. Contract tests fail if the response, pin,
source call graph, sample summaries, outcome or sanitization drifts.

Decision: D23/§6.3 must be revisited before any `botkit` conversation code.
D24 was not reached and remains unevaluated. In required measurement order,
the two-profile multiplex gateway, Pi RSS/latency, credential/profile/tool/socket
isolation, dedicated-user Linux sandbox, gym MCP attachment and prompt-size
phases were intentionally not run. No fallback topology is selected without
that evidence. See [`HERMES_SPIKE.md`](HERMES_SPIKE.md) for method, exact
limitations and the first blocked Pi rerun command.

Crates.io owner lookup was attempted with `cargo owner --list nswarm`; the
registry required an owner token and no credential was supplied. Reserving or
publishing `nswarm` remains a separate owner action and did not block this safe
spike work.

Local validation passed the exact source verifier, eleven evidence-integrity
and derivation checks, policy,
profile and generated-file checks, Fleet validation, formatting, check, eval,
Clippy, all 131 Rust tests, rustdoc, feature-power-set checks, deny, vet,
machete, coverage and semver unit tests. Repository coverage was 97.02%, with
296/296 marked critical branch outcomes covered. The previously red standard
semver comparison is repaired by applying Cargo's MSRV-aware resolver fallback
inside the semver runner. Its unlocked scratch packages now select transitive
versions compatible with Rust 1.90; the comparison passes all four shared
crates locally without changing the workspace lock, MSRV, or semver policy.

The full source verifier now compares the annotated tag object as well as the
peeled commit and source bytes. Committed checks exercise tag-object drift and
evidence derivation, but core CI deliberately does not fetch the external
Hermes repository; exact source-byte/call-graph verification remains a required
run against a checked-out source tree.

## Checkpoint Y — post-Step 3 follow-ups

PR #3 was verified merged into protected `origin/main` at
`c8c1b69390f607be7f0ae895441b2141c74a3d93` before this branch was created.
The missing protected-CI invocation identified in PR #3 review is repaired:
`scripts/test_hermes_gateway_spike.py` now runs near the start of the required
`quality` job, before the Rust build and test stages. This makes the eleven
deterministic pin, capability-response, source-anchor, sample-summary, outcome,
and sanitization checks fast required PR evidence rather than a local-only
`just ci` gate. The full external source-byte verifier remains intentionally
manual because ordinary CI does not fetch Hermes.

The stored `body_metrics.date` policy is now explicit and tested. Every row in
the bounded indexed candidate window must parse as RFC 3339 with an explicit
offset. One invalid selected row fails the whole query and MCP returns a generic
storage error; valid rows are not returned as silent partial results. An
offset-free value that a legacy naive Python `datetime.isoformat()` could have
written is therefore unsupported. Rows lexically outside the candidate window
are not audited by that call. Focused tests cover both the fail-closed selected
row and the bounded-query distinction.

The frozen v0 source does not prove that production data satisfies this policy.
Its batch and Apple Health paths accept `datetime` values without requiring an
offset and persist `.isoformat()` unchanged; v0 tests include offset-free Apple
Health timestamps. Before Step 4 uses a production database copy, run this
read-only corpus check on the Pi:

```sql
SELECT COUNT(*) FROM body_metrics
WHERE date NOT GLOB '*[+-][0-9][0-9]:[0-9][0-9]'
  AND date NOT GLOB '*Z';
```

A non-zero result blocks cutover readiness: add representative sanitized rows
to the fixture and define a reviewed migration before running v1 against the
copy. No production-corpus compliance or migration is claimed by this PR.

The mutation-driven database-flag spellings are retained without exclusions.
Named database flag constants and adjacent comments explain that
`OpenFlags::union` avoids redundant bitwise-operator mutants while preserving
mutation coverage around database validation. The parity snapshot uses the same
equivalent flag case. The inaccurate rationale beside exact path `.eq()` was
removed: its `==` to `!=` mutant is meaningful and the exact-path test already
kills it. No function-level mutation skip was added.

The Step 3 semver repair remains the single implementation. The runner applies
Cargo's MSRV-aware incompatible-Rust-version fallback only to its unlocked
scratch packages, so Rust 1.90 comparisons select compatible transitives without
changing the workspace lockfile, MSRV, or compatibility policy. This cleanup
duplicates no resolver logic; the Hermes evidence tests remain an independent
early step in the required `quality` job.

Chrono-to-Jiff was measured and rejected for this cleanup. The locked normal
`gym-bot` graph has 177 unique entries. It resolves one `chrono 0.4.45` and one
`jiff 0.2.35`, with no duplicate time-library versions; reverse-dependency
inspection shows Chrono is still required by `rmcp 3.1.4`, `schemars 1.2.2`, and
`teloxide-core 0.13.0`. Removing the direct gym Chrono edge would therefore
remove no crate, exemption, or duplicate and offers no demonstrated clean-build
benefit. An incremental `cargo check --locked -p gym-bot` completed in 6.83
seconds Cargo time (7.41 seconds wall, 134.25 MiB maximum RSS), while existing
cached time-library artifacts totalled about 32 MiB across profiles/features;
neither is clean attribution to a removable direct edge. Migrating also risks
changing RFC-3339 acceptance, calendar-day subtraction across DST, formatting,
and the public parse-error type. Both libraries are retained.

The read-only `cargo owner --list nswarm` lookup failed exactly with `no token
found`; no authentication, owner credential, reservation, or publication was
attempted. The separate owner action remains: authenticate explicitly and
publish a compliant first release to reserve `nswarm`.

Branch cleanup removed nothing. `codex/step2-gym-spike`,
`codex/step3-hermes-spike`, and `overnight/bootstrap` are squash-merged PR heads
whose original commits are not ancestors of `origin/main`, so they fail the
strict fully-merged ancestry rule. Local `codex/workflow-bootstrap` is an
ancestor of `origin/main` but is checked out in a separate worktree and was
preserved rather than altering that workspace. `main` and the open PR branch
remain protected.
