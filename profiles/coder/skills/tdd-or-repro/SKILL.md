---
name: tdd-or-repro
description: Establish red-before and green-after evidence for bugs and behaviour changes.
policy-version: v1
---

For a bug, reproduce the failure at the real contract boundary before editing
when feasible, and capture the red command and distinguishing failure. For a
feature or refactor, write the smallest observable acceptance proof first and
show that it would fail without the new behavior. Security and migration units
include negative tests for unauthorized, malformed, stale, partial, and replay
paths as applicable.

Keep clocks, randomness, filesystem state, model providers, and networks
hermetic and deterministic. Never weaken an assertion, update a snapshot
blindly, add a retry, exclusion, blanket allow, or compatibility branch merely
to turn red into green. Fix the root cause, retain red-before/green-after
evidence, and map the final tests to the immutable acceptance criteria.
