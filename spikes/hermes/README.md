# Hermes gateway spike

This directory pins the upstream input for §12 Step 3. The deploy revision is
the repository-reviewed `v2026.8.19` tag, not upstream `main`. `pin.json`
records the peeled commit, package metadata, source provenance, and hashes of
every file used by the architecture-gate harness.

Verify an already checked-out source tree without installing Hermes:

```console
python3 scripts/hermes_gateway_spike.py verify-source \
  --source /path/to/hermes-agent
```

The verifier fails closed on a different Git commit, package version, Python
requirement, or relevant source byte. It also checks the load-bearing call
path: session chat delegates to `_run_agent`, `_run_agent` constructs an agent
for each request, and only the native messaging gateway contains the live-agent
cache.

No profile state, credentials, transcripts, caches, or owner-specific paths are
stored here. Runtime measurements and their limitations are recorded separately
after the source gate passes.
