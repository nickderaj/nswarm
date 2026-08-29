# Branch protection

An authenticated administrator applied `scripts/configure_branch_protection.sh`
to `main` on 2026-08-28 after the repaired exact head passed every PR and
merge-tier job. The configuration was then read back through GitHub's API; the
returned state is recorded in `docs/BUILD_STATUS.md` rather than inferred from
this template.

Required policy for `main`:

- no direct push, force push, deletion, or administrator bypass;
- linear history;
- branch must be current and all conversations resolved;
- merge queue required when GitHub supports it for the repository owner/plan;
- PR-tier and exact-head merge-tier jobs all green; nightly failures are
  separately monitored and repaired;
- skipped, neutral, pending, or stale checks do not satisfy the rule;
- policy paths in `CODEOWNERS` require owner review;
- agents can update their own PR branch but cannot merge or alter protection.

The setup script is deliberately not called from CI and never merges work.
Required checks are bound to GitHub Actions app id `15368`, so a status from a
different integration cannot satisfy the rule. This personal public repository
does not support GitHub merge queues; GitHub documents that public-repository
merge queues require organization ownership. Until the repository moves to an
organization, the same merge-tier workflow runs on `pull_request` so its jobs
are reported to the PR's required-check rollup. It also listens for
`merge_group` so no workflow change is needed after a future move enables a
merge queue. `workflow_dispatch` remains available for diagnostics, but a
manual run is not used to satisfy the PR gate.

The API rejects an empty legacy `contexts` array when an app-bound `checks`
array is supplied, so the template uses only `checks`. GitHub reports
`allow_fork_syncing` disabled for this branch; the template matches that actual
state.

Each required context is the exact `check_run.name` returned by GitHub's API.
The Actions UI may display a composite workflow/job label such as
`PR / quality`, but the corresponding required context is `quality`. Requiring
the composite display label leaves the rule permanently waiting for a status
that no check run reports.

Required signed commits are not enabled in this bootstrap. The existing
`overnight/bootstrap` history predates a signing policy and contains unsigned
commits; GitHub's signed-commit rule would therefore require rewriting that
history, which this build explicitly forbids. Enable the rule only through a
separately reviewed migration after all active branches comply, without
rewriting or force-pushing this branch.
