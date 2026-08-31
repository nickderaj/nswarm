---
name: why
description: Investigate intent across code, history, issues, documents, chat, and observability.
policy-version: v1
---

Anchor the present behaviour in revision-pinned source code before looking for
rationale. Then search each brief-authorized category independently: source
control history, issues, long-form documents, chat, observability, error
tracking, and analytics. Do not let a persuasive result in one category stand
in for checking another category named by the brief.

For source control, inspect the introducing change and relevant later edits.
For issues, documents, and chat, distinguish contemporaneous decisions from
later recollection. For observability, error tracking, and analytics, state the
environment and time window. Current code proves what happens now; it does not
prove why the choice was made.

Record every category as searched, empty, unavailable, or skipped. A skipped
category needs an explicit reason tied to the brief; an unavailable category
must name the missing structural grant or service. Seek contradictory evidence
and preserve it in the claim manifest instead of forcing one tidy narrative.
