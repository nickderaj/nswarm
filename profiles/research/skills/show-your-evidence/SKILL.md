---
name: show-your-evidence
description: Emit the machine-readable claim and source manifest required by the brief.
policy-version: v1
---

Return each claim with `kind`, `text`, `source_type`, `revision`, `location`,
`observed_at`, `confidence`, and `limitations`. A `direct` claim without a
reachable citation is invalid. Redact secret-like values, validate the report
schema, and include searched, empty, unavailable, and skipped source lists.
