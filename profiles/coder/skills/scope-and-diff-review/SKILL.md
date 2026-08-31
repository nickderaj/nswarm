---
name: scope-and-diff-review
description: Review every changed line, generated file, and path against the immutable brief.
policy-version: v1
---

Before handoff, inspect repository status, every staged and unstaged line,
untracked files, submodules, generated artifacts, schema and snapshot changes,
dependency and supply-chain records, and the exact base-to-head diff. Map every
changed path to the brief's writable scope and an acceptance criterion.

Reject scope creep, sibling access, secret-like values, policy or CI weakening,
baseline churn, blanket suppressions, opportunistic dependency updates,
compatibility code without a current consumer, and unrelated cleanup. Keep the
candidate atomic. If a generated or shared file is necessarily touched, name
its owning generator and evidence rather than treating it as incidental.
