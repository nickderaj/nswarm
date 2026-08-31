---
name: verify-real-artifact
description: Test the built or rendered artifact that a caller will actually consume.
policy-version: v1
---

Verify the binary, database, rendered unit, schema, package, socket contract, or
generated file that the caller will actually consume, not only an internal
helper. Build from the committed candidate and confirm the worktree head equals
the reported exact SHA before collecting evidence.

Run every literal brief command in order without a shell, ambient credential,
silent skip, retry, or substituted flag. Capture command argv, exit status,
relevant behavior, redacted output digest, artifact path and digest, base SHA,
and head SHA. A changed head invalidates all prior evidence.
