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

After installing the pinned lockfile into an isolated Python 3.11–3.13
environment, run the authenticated loopback route probe with:

```console
/path/to/pinned/venv/bin/python scripts/hermes_gateway_spike.py measure-reuse \
  --source /path/to/hermes-agent --samples 30 --output measurement.json
```

The route probe uses explicit durable session IDs and a deterministic fake
provider boundary. It measures only the HTTP/session/SQLite/executor route with
`time.perf_counter_ns`; it does not claim live-provider or provider-side prompt
cache latency. The first global route prime is reported separately, then 30
new-session calls and 30 repeated calls to one explicit session are recorded.

The verifier fails closed on a different Git commit, package version, Python
requirement, or relevant source byte. It also checks the load-bearing call
path: session chat delegates to `_run_agent`, `_run_agent` constructs an agent
for each request, and only the native messaging gateway contains the live-agent
cache.

No profile state, credentials, transcripts, caches, or owner-specific paths are
stored here. Runtime measurements and their limitations are recorded separately
after the source gate passes.

The committed result is `evidence/http-reuse.json`. It confirms that every
request, including all repeated calls to the same session ID, invokes the agent
factory and receives a distinct agent instance. The session transcript and
system prompt can be restored from SQLite, and MCP discovery is process-wide,
but the `AIAgent`, memory load, tool snapshot and in-memory session state are
reconstructed. This fails the D23 architecture gate, so the multiplexing/Pi,
credential-isolation, socket-ACL and prompt-size phases are intentionally not
executed until §6.3 is revisited.
