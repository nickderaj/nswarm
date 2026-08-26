---
name: pause-and-handoff
description: Stop at an atomic boundary and preserve durable state when work cannot continue.
policy-version: v1
---

Finish or revert the smallest incomplete edit without touching unrelated work.
Record current state, exact SHA, completed checks, captured failures, blockers,
uncommitted paths, and the next executable command. Never commit a knowingly
broken intermediate state merely because time expired.
