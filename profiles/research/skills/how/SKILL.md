---
name: how
description: Trace current behaviour from input to output with revision-pinned code evidence.
policy-version: v1
---

Trace the real entry point through state, configuration, side effects, and
caller-facing output. Prefer executing a hermetic proof over inferring from
names. Cite repository, exact commit, path, symbol, and line span. Separate an
observed path from an unexecuted alternative and report unknown runtime state.
