---
name: architect
description: Define data shapes, invariants, and caller-facing interfaces before cross-boundary edits.
policy-version: v1
---

Name the input and output shapes, ownership, lifetime, trust boundary, failure
states, persistence format, migration path, concurrency rules, recovery, and
caller compatibility before changing a function, file, process, or schema
boundary. Identify which invariants types enforce and which need runtime proof.

Prefer an existing seam, a smaller representation, and deletion over a new
abstraction. Every abstraction needs a concrete current consumer in the leased
unit. List negative cases and show how illegal states, partial writes, stale
identity, and unauthorized callers are rejected mechanically.
