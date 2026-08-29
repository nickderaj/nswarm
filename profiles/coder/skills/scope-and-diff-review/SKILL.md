---
name: scope-and-diff-review
description: Review every changed line, generated file, and path against the immutable brief.
policy-version: v1
---

Before handoff, inspect status, the complete diff, untracked files, generated
artifacts, and dependency changes. Reject scope creep, sibling access, secrets,
policy weakening, blanket suppressions, and unrelated cleanup. Keep the
candidate atomic and explain any necessary exception with evidence.
