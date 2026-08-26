# Branch protection

GitHub administration is an external, owner-authorized step. Until an
authenticated administrator applies `scripts/configure_branch_protection.sh`,
the repository configuration is **pending** even when all local checks pass.

Required policy for `main`:

- no direct push, force push, deletion, or administrator bypass;
- signed commits and linear history;
- branch must be current and all conversations resolved;
- merge queue required;
- PR-tier jobs, merge-tier jobs, and current default-branch nightly all green;
- skipped, neutral, pending, or stale checks do not satisfy the rule;
- policy paths in `CODEOWNERS` require owner review;
- agents can update their own PR branch but cannot merge or alter protection.

The setup script is deliberately not called from CI and never merges work.
