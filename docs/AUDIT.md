# Overnight implementation audit

Audit date: 2026-08-27 (Europe/London)

Scope: complete `main...overnight/bootstrap` history and diff, the executable
CI contract, dependency policy, profiles and eval corpus, generated artifacts,
and the step-1 control-plane implementation. The branch was treated as
untrusted; prior statements in `docs/BUILD_STATUS.md` were not accepted as
evidence.

## Independent evidence

- Audited branch: `095fd42d7fd2c954fb46752e9f63c5b177f094ec`.
- Default branch: `70fb4af` (`Initial commit`), containing no workflow files.
- GitHub reported zero recognized workflows, zero check runs, no PR, and no
  protection for `main` at the start of this audit.
- A detached clean worktree at the audited SHA failed `just ci` immediately.
  `scripts/policy_check.py` rejected all protected paths because
  `git branch --show-current` is empty in detached checkouts. The earlier local
  pass depended on the literal branch-name exemption `overnight/bootstrap`.
- The pinned `nightly-2025-09-18` exists and installs `miri`, `rust-src`, and
  `llvm-tools-preview`. The configured `cargo miri test --workspace` command
  is not viable: it fails when the provisioner test performs isolated
  filesystem operations.
- Independent nightly branch coverage was 60.78% (265/436), despite the line
  gate passing at 92.82% (3192/3439). Per-file branch coverage included
  `policy.rs` 0%, `store.rs` 68.02%, `types.rs` 52.5%, `botkit` 50%, and the
  `fleet` binary 25%. The current CI does not measure branch coverage.
- At the audited SHA, `actionlint` 1.7.12 parsed the workflows and diagnosed the
  undeclared custom `nswarm-pi` runner label. The repaired workflows pass
  actionlint without diagnostics. GitHub currently reports no registered
  repository runners.
- `actions/checkout@11bd719...` is a real commit, but it is not current. The
  current immutable `v7.0.1` commit is `3d3c42e5aac5ba805825da76410c181273ba90b1`.

## Critical findings

### C-1: PR CI cannot pass in a normal checkout

The policy-change gate authorizes changes using a mutable local branch name
and rejects a detached checkout. GitHub checks out pull-request merge commits
without relying on that branch name, so the advertised PR workflow fails
before compilation. Conversely, a contributor can name a branch `policy/*`
and bypass the local authorization decision. Content validation belongs in the
repository checker; policy-change authorization must be enforced by protected
GitHub CODEOWNER review, not by a spoofable checkout name or the
`NSWARM_POLICY_CHANGE_ALLOWED` environment variable.

### C-2: The public-repository Pi job is not a safe trust boundary

The repository is public and owned by a personal account. A pull request can
introduce or modify a workflow that targets any repository-level self-hosted
runner label before its code is trusted. Personal repositories do not provide
an organization runner group restricted to a selected workflow pinned to
`refs/heads/main`. A YAML event condition in the existing workflow cannot
protect a runner from a malicious replacement workflow. No Pi runner is
currently registered, which is the safe state. The self-hosted job must not be
enabled until the runner is placed behind a workflow-restricted organization
runner group (or an equivalently isolated, ephemeral broker). Hosted ARM64 can
provide merge-tier execution in the meantime; this is not evidence of physical
Pi execution.

### C-3: Required branch coverage is absent and the implementation is far below it

At the audited SHA, the PR coverage job checked repository line coverage only.
It did not report branches, enforce per-crate coverage, enforce changed-line
coverage, or prove 100% branch coverage for authorization/security paths as
required by plan section 8.2.1. Adding the missing measurement without
additional tests would correctly fail at 60.78% branch coverage.

## High findings

### H-1: Merge-tier commands are configured but not operational

`cargo miri test --workspace` fails on ordinary filesystem/process tests under
Miri isolation. The sanitizer commands omit the strongly recommended
instrumented standard-library build (`-Zbuild-std`) and do not isolate ASan and
LSan target directories. Mutation uses `--in-place`; an interruption can leave
the source checkout modified. No merge job has an explicit timeout or uploads
its test/mutation output.

### H-2: Role capabilities are not consistently enforced by store operations

Roles and capabilities exist as types, and reviewer/integrator/coordinator
checks exist for a subset of repository methods. However merge authorization
accepts an arbitrary non-empty `authorized_by` string rather than a live
same-job shipper profile. Other lifecycle methods describe an owning role in
prose but accept no actor identity. The current API therefore cannot prove the
claimed role-to-capability boundary end to end.

### H-3: Path leases accept aliasing and traversal forms

Path overlap uses lexical `Path::starts_with`, but lease resources are only
checked for non-empty text. Values containing absolute paths, `..`, or other
non-canonical aliases can describe the same resource while comparing as
non-overlapping. Logical repository lease paths must be normalized,
repository-relative component sequences before persistence and comparison.

### H-4: Secret redaction is incomplete

Object-key redaction recognizes exact names or underscore suffixes, so common
forms such as `apiKey`, `accessToken`, and `clientSecret` are retained. String
redaction recognizes only classic `ghp_` and `sk-` shapes; modern GitHub token
families such as `github_pat_` and `gho_`, bearer authorization values, and
other common credential forms are missed. This contradicts the shared
persistence-boundary claim.

### H-5: Generated/profile drift checks do not compare the complete tree

`git diff -- generated profiles` ignores untracked files and separately staged
changes. The profile validator checks two expected generated files but does not
reject extra tracked generated profiles. Symlinks are not rejected. A clean
tree comparison must use the index/worktree/untracked status and exact expected
inventories.

### H-6: A stale worker can quarantine a replacement worker's unit

When a result arrives for an expired lease, `accept_worker_result` transitions
the unit to `quarantined` even if a newer live lease now owns the work. The late
result should be quarantined durably without revoking the replacement lease's
current unit state.

## Medium findings

### M-1: Policy scans have syntax and file-shape bypasses

The ignored-test expression misses `cfg_attr(..., ignore)`. Network-test checks
only run when the same source file contains the literal `#[cfg(test)]`, so an
integration test can use the network. A new workspace crate can omit
`[lints] workspace = true`, avoiding the compiler's `unsafe_code = "forbid"`
contract. Repository symlinks and alternate protected content shapes are not
rejected.

### M-2: Operation-level idempotency is not established

The append-only event helper is idempotent, but most state-changing methods
perform state precondition checks before replay detection. Retrying a completed
transition or merge authorization with the same key generally returns an
invalid-transition error rather than the original success/result. The current
evidence supports ledger-event idempotency only, not command idempotency.

### M-3: CI omits parts of the declared plan contract

The PR tier lacks bounded fuzzing and clean-build reproducibility, does not use
the configured nextest CI/JUnit profile, and uploads no artifacts. Feature
testing does not explicitly run every package with no default features. Jobs
lack concurrency cancellation and timeouts. The proposed protection contexts
omit `PR / current-stable` and require merge contexts before any such contexts
have appeared, which would deadlock if applied now.

### M-4: `store.rs` has genuinely mixed responsibilities

At 104,473 bytes and 2,914 lines, the file combines schema DDL and migrations,
job/state repositories, leases, profiles/sessions/credentials, branches and
artifacts, review gates, redaction, error definitions, and roughly 1,100 lines
of tests. Transaction helpers avoid some duplication and the invariants are
mostly colocated coherently, so size alone is not a reason to split it. Schema
migrations and evidence redaction are independently composable boundaries;
the domain repositories should be split only alongside tests that preserve
cross-repository transaction invariants.

### M-5: Supply-chain acceptance is exemption-only

All 38 cargo-vet entries are exemptions and `audits.toml` contains no audits.
The configuration fails closed for new dependencies, which is good, but no
crate has yet been reviewed strongly enough to replace an exemption. Direct and
security-sensitive dependencies must be inventoried before any exemption is
removed; no audit may be inferred from a passing build.

## Follow-up independent review disposition

A second independent review was received on 2026-08-27 after checkpoint K.
The findings below were reproduced against the code rather than accepted from
the report at face value.

### Critical follow-up findings

- **C-4 — the data model cannot represent a multi-unit job and dependencies
  fail open.** `jobs.brief_json` contains one unit-specific brief, `job_id` is
  unique at creation, and the dependency query uses an inner join. A dependency
  identifier with no matching unit therefore disappears from the unsatisfied
  count. Repair requires a schema migration that separates job-level identity
  from immutable unit briefs, foreign-keys both dependency endpoints, and
  proves two units in one job plus unknown-dependency rejection.
- **C-5 — verification verdicts are unattributed.** A caller can record a
  verdict without a live verifier profile, while review findings do carry an
  actor. Verdict persistence and acceptance must bind evidence to an explicit,
  live, same-job verifier/reviewer capability at the exact SHA.

### High follow-up findings

- **H-7 — generated unit inventory was incomplete. Repaired in the current
  worktree.** `scripts/generate.sh` named `research.toml` directly. Generation
  now derives every bot service from the validated manifest inventory and a
  two-manifest regression proves omission is impossible.
- **H-8 — critical coverage selection failed open. Repaired in the current
  worktree.** The checker used a hand-maintained list of mangled-name
  substrings, silently treated zero totals as 100%, and had no minimum match
  count. Source-level `// coverage-critical` markers now define the inventory,
  at least 30 functions must compile and match structurally, zero branch totals
  fail, duplicate binary copies are safely aggregated, and every missing
  outcome is reported with file, line, and function. The fresh local result is
  232/232 critical outcomes across 34 functions.
- **H-9 — gateway credential policy used a denylist. Repaired in the current
  worktree.** Only `OPENROUTER_API_KEY` and `XAI_API_KEY` are accepted; synthetic
  `ERROR_BOT_TOKEN`, `LOG_BOT_TOKEN`, `GITHUB_TOKEN`, and operational bot-token
  cases are rejected.
- **H-10 — Git subprocesses inherited ambient configuration, hooks, and
  credentials. Repaired in the current worktree.** Provisioning now clears the
  process environment, supplies only a fixed system path and inert home/config
  locations, disables system/global Git config, credential helpers, prompts,
  hooks, and fsmonitor, and prohibits the file transport. A hostile executable
  `post-checkout` hook is proven not to run.
- **H-11 — merge-authorized/integrated recovery paths are incomplete.** The
  state machine and unique merge-authorization row can strand a unit when a
  protected merge is rejected or its candidate changes. Exact-SHA
  invalidation and explicit recovery transitions need durable tests.
- **H-12 — capabilities were not the store's authorization source of truth.
  Partially repaired in the current worktree.** Existing actor-bearing merge,
  coordinator, reviewer, and integrator operations now resolve a live profile's
  persisted role through the typed `Role` parser and capability map. Structured
  profile policy no longer invents a `deploy` token absent from the Rust enum.
  Lifecycle methods without actors and verdict attribution remain active under
  C-5.
- **H-13 — topology leases were globally exclusive. Repaired in the current
  worktree.** Topology conflicts are now job-scoped; same-job concurrent owners
  fail while two independent jobs can hold integration topology concurrently.
- **H-14 — evals are partially tautological.** The Python runner duplicates
  simplified redaction/state logic and the corpus contains prose-like snippets
  instead of driving the Rust implementation. The corpus must invoke named
  hermetic Rust tests and fail if those implementation tests do not run.
- **H-15 — host sandbox proof remains incomplete. Partially repaired in the
  current worktree.** Rendered bot units now add `UMask=0077`,
  `SystemCallFilter=@system-service`, and systemd IP allow/deny rules that
  permit loopback Hermes access while denying non-loopback IP traffic. Secret
  environment values ending in a continuation backslash are rejected, and
  deny paths cannot be ancestors of writable roots. Socket group/ACL rendering
  and real systemd enforcement remain unmeasured and must not be claimed.

### Medium follow-up findings

- **M-6 — actionlint was version-tag pinned rather than commit pinned. Repaired
  in the current worktree.** Official tag `v1.7.12` resolves to upstream commit
  `914e7df21a07ef503a81201c76d2b11c789d3fca`; the workflow uses that immutable
  revision and policy rejects non-40-hex Go tool revisions.
- **M-7 — CI repeatedly compiles installed tools.** Safe compiler/dependency and
  version-keyed binary caching is desirable, but may not cache credentials and
  must not weaken immutable tool selection.
- **M-8 — several declared future surfaces are placeholders.** Generated
  profiles, Python runtime policy, and initial skill files are not evidence of
  runtime wiring. These remain implementation work rather than completed
  capabilities.

The review's observation that `store.rs` mixes schema, persistence domains,
redaction, and tests agrees with M-4. Refactoring remains subordinate to the
multi-unit/verdict migrations: the file will be split only at transactionally
composable boundaries, with cross-domain invariant tests retained.

## Positive observations

- Exact full SHA types, branch-head compare-and-swap, exact-SHA verdict lookup,
  integration reverification, and exact-SHA merge-record checks are present and
  have useful negative tests.
- Lease acquisition uses an immediate SQLite transaction and expires old rows
  before overlap checks, preventing concurrent check/insert races within this
  store connection model.
- Evidence/event updates are transactional; an idempotency conflict rolls back
  surrounding state changes.
- Worktree creation uses direct argument vectors rather than a shell and
  canonicalizes the source repository and destination parent. The final
  destination still needs explicit dangling-symlink rejection and the allowed
  root must remain scheduler-owned to bound the remaining check/use race.
- Workflow-level permissions are read-only and fork PRs currently target only
  GitHub-hosted jobs. Action references are immutable full SHAs.
- Ordinary Rust tests are hermetic; the observed tests did not require live
  Telegram, model, host, credential, or gym resources.

## Plan comparison

The branch implements a substantial step-1 skeleton from sections 7.5 and
8.2.1, but it is not a completed step-1 baseline. Local line coverage and unit
tests do not substitute for recognized GitHub checks, branch coverage,
operational merge tools, protected review, or safe runner isolation. Step 2
must not begin until the real PR checks are green and the protection result has
been queried back from GitHub.
