---
name: show-me-your-work
description: Emit an exact-SHA candidate report with commands, artifacts, risks, and limitations.
policy-version: v1
---

Return schema version, repository, exact base SHA, exact committed head SHA,
every changed path, one-to-one acceptance evidence, literal command argv and
exit results, redacted output digests, artifact types/paths/SHA-256 digests,
remaining risks, and deviations. Changed paths must be unique and inside the
brief's writable scope; commands must match the brief in order and exit zero;
every artifact must be attributed to the reported head.

Inspect the final diff and verify the local head before serializing the report.
Use explicit `none` entries only after assessing risks and deviations. Reject
policy-shaped extra fields and validate the machine-readable schema. Worker
self-report is untrusted evidence: it can create a candidate, but never marks
that candidate verified, integrated, authorized, merged, or deployed.
