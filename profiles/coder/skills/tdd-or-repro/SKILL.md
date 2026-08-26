---
name: tdd-or-repro
description: Establish red-before and green-after evidence for bugs and behaviour changes.
policy-version: v1
---

For a bug, reproduce the failure before editing when feasible. For a feature,
write the smallest observable acceptance proof first. Keep fixtures hermetic and
deterministic. Never weaken an assertion to turn red into green; fix the root
cause and retain both negative and positive coverage.
