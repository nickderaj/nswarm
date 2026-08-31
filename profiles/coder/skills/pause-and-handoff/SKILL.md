---
name: pause-and-handoff
description: Stop at an atomic boundary and preserve durable state when work cannot continue.
policy-version: v1
---

Stop at the smallest atomic boundary. Finish or revert the current incomplete
edit without touching unrelated or pre-existing work, then inspect status and
the full diff. Do not commit a knowingly broken intermediate state merely
because time or budget expired.

Return a schema-valid handoff containing repository, base and current exact SHA,
branch/worktree identity, completed acceptance criteria, literal commands and
results, captured failures, remaining risks, blocker category, uncommitted and
untracked paths, credential/lease expiry, and the next executable command. A
missing secret, external approval, architecture decision, or expanded authority
is reported precisely and never worked around.
