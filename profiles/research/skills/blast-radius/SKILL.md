---
name: blast-radius
description: Identify callers, state, contracts, and safety facts affected by a proposed change.
policy-version: v1
---

Name the safety facts on which the change depends. Trace direct and indirect
callers, persisted data, schemas, generated outputs, deployment surfaces, and
failure recovery. Use repository search and executable probes; do not assert
that an untested path is safe. Mark every unverified edge as inferred or
unknown.
