---
name: blast-radius
description: Identify callers, state, contracts, and safety facts affected by a proposed change.
policy-version: v1
---

Name the safety facts on which the proposed change depends, including trust,
identity, persistence, ordering, idempotency, compatibility, and recovery
assumptions. Trace direct and indirect callers, public types, persisted data,
schemas and migrations, generated outputs, manifests, deployment surfaces,
credentials, socket or network boundaries, and failure recovery.

Search for consumers by symbol, serialized field, command, path, and generated
artifact rather than relying on one reference search. Execute the smallest
read-only probe that can validate a material edge. Classify every edge as
direct, inferred, contradicted, or unknown, and state the exact next proof for
anything whose failure could change the recommendation.
