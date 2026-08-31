---
name: blast-radius
description: Enumerate affected callers, schemas, generated artifacts, and security boundaries.
policy-version: v1
---

Name every safety assumption, then search for direct and indirect callers,
serialized fields, persisted formats, schemas and migrations, manifests,
snapshots, generated output, dependency and supply-chain records,
documentation, deployment policy, socket/network boundaries, and rollback.

Search by symbol, command, field, path, and generated artifact so one reference
index cannot hide a consumer. Execute the real path where practical and mark
each unverified edge as inferred or unknown. Do not claim an uninspected or
untested edge is safe.
