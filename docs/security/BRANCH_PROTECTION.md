# Branch protection

GitHub administration is an external, owner-authorized step. Until an
authenticated administrator applies `scripts/configure_branch_protection.sh`,
the repository configuration is **pending** even when all local checks pass.

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
organization, the same merge-tier workflow is dispatched manually at the exact
PR head SHA and remains required by branch protection.

Required signed commits are not enabled in this bootstrap. The existing
`overnight/bootstrap` history predates a signing policy and contains unsigned
commits; GitHub's signed-commit rule would therefore require rewriting that
history, which this build explicitly forbids. Enable the rule only through a
separately reviewed migration after all active branches comply, without
rewriting or force-pushing this branch.
