---
name: show-your-evidence
description: Emit the machine-readable claim and source manifest required by the brief.
policy-version: v1
---

Return schema version, the exact brief question, the declared done predicate,
report limitations, and a normalized claim list. Every claim contains `kind`,
`text`, `source_type`, `revision`, `location`, `observed_at`, `confidence`, and
`limitations`. Allowed kinds are `direct`, `inferred`, `contradicted`, and
`unknown`; an unknown claim uses confidence `none`.

A direct, inferred, or contradicted claim must cite a source class actually
searched. A direct claim without a reachable revision-pinned citation is
invalid. Use repository commit plus path/symbol/lines or an immutable external
reference; never cite a search result page or an unresolvable label.

Account for every brief-required source class exactly once as searched, empty,
unavailable, or skipped. Keep unsupported conclusions as unknown rather than
promoting absence of evidence. Redact secret-like values, reject policy-shaped
extra fields, validate the machine-readable report schema, and ensure the human
summary does not exceed the claims it contains.
