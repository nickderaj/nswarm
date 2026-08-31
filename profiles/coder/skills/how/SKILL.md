---
name: how
description: Trace the current code path and contracts before modifying them.
policy-version: v1
---

Resolve the exact base revision, then locate the real entry point, direct and
indirect callers, public types, ownership, configuration, state reads and
writes, side effects, error mapping, and caller-facing output. Inspect the
tests, but do not treat a test name as proof that production uses the path.

Run the smallest hermetic proof that distinguishes observed behaviour from the
current hypothesis. Record the literal command, fixture, exit result, exact
paths, symbols, and branches not executed. Do not edit until the relevant path
and its writable boundary have been inspected.
