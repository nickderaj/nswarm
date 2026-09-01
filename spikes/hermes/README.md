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
`time.perf_counter_ns`; it does not claim actual `AIAgent` construction,
growing-transcript, live-provider, or provider-side prompt-cache latency. The
first global route prime is reported separately, then 30 new-session calls and
30 repeated calls to one explicit session are recorded.

The verifier fails closed on a different Git commit, annotated tag object,
package version, Python requirement, or relevant source byte. It also checks
the load-bearing call path: session chat delegates to `_run_agent`, `_run_agent`
constructs an agent for each request, only native messaging holds a live-agent
cache, and the provider cache planner/usage path remains intact.

No profile state, credentials, transcripts, caches, or owner-specific paths are
stored here. Runtime measurements and their limitations are recorded separately
after the source gate passes.

The HTTP result is `evidence/http-reuse.json`. It confirms that every
request, including all repeated calls to the same session ID, invokes the agent
factory and receives a distinct agent instance. The session transcript and
system prompt can be restored from SQLite, and MCP discovery is process-wide,
but the `AIAgent`, memory load, tool snapshot and in-memory session state are
reconstructed. This fails the D23 architecture gate, so the multiplexing/Pi,
credential-isolation, socket-ACL and prompt-size phases are intentionally not
executed until §6.3 is revisited.

The native-adapter follow-up is `evidence/native-adapter.json`. Native messaging
reuses cached agents, but only behind first-class platform adapters and private
Python ingress methods. That would move Telegram out of `botkit` and does not
provide synchronous `ask()`. Hermes Relay retains native cache reuse while
separating transport ownership, but its protocol is explicitly experimental and
requires a connector outside the reviewed pin. Neither path is a reviewed D23
local-agent-cache replacement. The stable HTTP transport remains a candidate;
the separate provider-cache experiment settles that remaining D23 gate.

Run the paid provider-cache harness only with an operator-selected ceiling and
explicit opt-in. The credential is read only from the environment:

```console
SURPLUS_API_KEY=... python3 scripts/hermes_provider_cache_spike.py \
  --source /path/to/hermes-agent \
  --output spikes/hermes/evidence/provider-cache.json \
  --turns 4 --model claude-haiku-4.5 --provider anthropic \
  --prefix-bytes 24576 --max-output-tokens 8 --max-spend-usd 1.00 \
  --i-understand-this-spends-money
```

`evidence/provider-cache.json` contains aggregate token buckets, modeled cost
and end-to-end latency only. It records 6,435 provider cache writes on the
long-lived prime and 6,435 cache reads on each of three repeats. Repeated-turn
modeled cost falls 91.02%, from 24,391 to 2,190 micro-USD. The API key, prompt,
transcript, response text, request identifiers and per-run cache nonces are not
stored. `scripts/check_hermes_provider_cache_evidence.py` derives the complete
result and fails closed in local, PR and merge-queue CI.

This live experiment revises D23 to retain the stable HTTP session route and
unblocks D24's multiplexing/isolation/Pi evaluation. It is not a bot session,
deployment, gym parallel trial or live pilot.
