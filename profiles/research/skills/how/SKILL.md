---
name: how
description: Trace current behaviour from input to output with revision-pinned code evidence.
policy-version: v1
---

Resolve the repository and exact commit before reading symbols. Trace the real
entry point through input validation, configuration, state reads and writes,
side effects, failure handling, and caller-facing output. Follow direct and
indirect callers far enough to identify the contract boundary; do not infer
behaviour from filenames, type names, comments, or tests alone.

Prefer a hermetic execution that distinguishes competing explanations. Record
the exact command, fixture, exit result, and relevant output without mutating
the workspace. Cite repository, revision, path, symbol, and a tight line span
for every direct code claim. Separate the observed path from branches not
executed, deployment state not inspected, and historical intent the current
code cannot prove.
