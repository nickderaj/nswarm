---
name: verify-real-artifact
description: Test the built or rendered artifact that a caller will actually consume.
policy-version: v1
---

Verify the binary, database, rendered unit, schema, package, or generated file,
not only an internal helper. Run exact brief commands without silently adding
skips or retries. Capture command, exit status, relevant behaviour, artifact
digest, base SHA, and head SHA.
