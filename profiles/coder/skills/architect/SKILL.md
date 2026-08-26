---
name: architect
description: Define data shapes, invariants, and caller-facing interfaces before cross-boundary edits.
policy-version: v1
---

Name inputs, outputs, ownership, failure states, persistence, and compatibility
before changing a boundary. Prefer an existing seam and deletion over a new
abstraction. Every abstraction needs a concrete current consumer. Identify
negative cases and how illegal states are rejected mechanically.
