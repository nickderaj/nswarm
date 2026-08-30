# Hermes gateway architecture-gate report

Status: D23 gate failed on 2026-08-30. D24 was not reached.

## Pinned provenance

The repository-reviewed deployment pin remains unchanged:

- source: `https://github.com/NousResearch/hermes-agent.git`;
- tag: `v2026.8.19`;
- annotated tag object: `b05e680e63d39d5a8e3ec0f5842a41d1c4209c03`;
- peeled commit: `fcbd1076a93841fa88855acce810e342a5b78101`;
- package: `hermes-agent==0.20.5`;
- Python requirement: `>=3.11,<3.14`;
- lockfile and all source files used by the harness are SHA-256 pinned in
  [`../spikes/hermes/pin.json`](../spikes/hermes/pin.json).

The design plan was researched at later source prefix `68518c1` with nearest
tag `v2026.8.19`. The executable spike uses the reviewed tag itself; it does not
follow that later commit or upstream `main`.

## Architecture result

The HTTP session route does not reuse a warm agent. In the pinned source:

1. `POST /api/sessions/{session_id}/chat` calls `_run_agent` exactly once
   (`gateway/platforms/api_server.py:3803`).
2. `_run_agent` calls `_create_agent` unconditionally inside its executor on
   every request (`api_server.py:6388`).
3. The API adapter has no agent cache. The native messaging path has the
   `_agent_cache` and its reuse logic, but the HTTP route does not enter it.

The distinction between durable and warm state matters:

- the transcript is loaded from `SessionDB` for each request;
- the first turn builds and persists the full system prompt; later turns can
  restore its exact bytes from `SessionDB` instead of rebuilding it;
- MCP discovery and live MCP server connections are process-wide and initiated
  during gateway startup;
- nevertheless, each HTTP request constructs a new `AIAgent`, takes a new tool
  snapshot, reloads enabled built-in memory, creates new in-memory todo/session
  state, and runs the turn on that new object.

Persisted transcript/prompt continuity may preserve an upstream provider's
byte-prefix cache. It is not evidence of the long-lived, in-memory `AIAgent`
reuse asserted by D23/§6.3 and cannot retain other reusable agent state.

## Executable trial

[`../scripts/hermes_gateway_spike.py`](../scripts/hermes_gateway_spike.py)
first verifies the exact Git/package/file pins and the source call graph. Its
route probe then exercises the real authenticated aiohttp handlers, real
`SessionDB`, explicit caller-assigned session IDs, and executor boundary. The
provider/agent boundary is a deterministic local fake so the probe can count
factory calls and object instances without a paid API or credential.

Environment:

- Apple arm64 host, Darwin 25.5.0;
- Python 3.12.12 in an isolated environment resolved from the pinned Hermes
  `uv.lock`;
- ephemeral loopback-only test listener and temporary Hermes state root;
- `time.perf_counter_ns`, a monotonic clock.

Method:

- one global route prime, reported separately;
- 30 first-chat requests, each against a newly created explicit session ID;
- one unrecorded prime plus 30 measured requests against one repeated explicit
  session ID;
- two persisted transcript messages inserted before the repeated samples to
  prove the route reloads durable history even while replacing the agent;
- exact `/v1/capabilities` response queried through the authenticated handler
  and bound to a canonical SHA-256 contract.

Results:

| Class | n | min | median | mean | p95 | max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| New explicit session | 30 | 0.348 ms | 0.403 ms | 0.405 ms | 0.474 ms | 0.542 ms |
| Repeated explicit session | 30 | 0.340 ms | 0.372 ms | 0.378 ms | 0.431 ms | 0.470 ms |

The separate global route prime was 84.470 ms. Across all 62 chat calls (route
prime, 30 new-session calls, repeated-session prime, and 30 repeated-session
calls), the agent factory ran 62 times. The 31 calls for the repeated session
created 31 distinct instances. The complete raw samples, environment,
capabilities response, source anchors, and excluded claims are in
[`../spikes/hermes/evidence/http-reuse.json`](../spikes/hermes/evidence/http-reuse.json).

These are route-overhead timings with a deterministic fake provider. They are
not live-model latency, actual `AIAgent` initialization latency, provider-side
prompt-cache latency, or Raspberry Pi latency. Their purpose is to prove the
factory and state lifecycle, which they do deterministically.

## Validation

The exact source verifier, eight spike regression tests, repository policy
check, formatting check, and diff hygiene pass locally. The standard `just ci`
run passed its policy, profile, generated-file, Fleet, formatting, check, eval,
Clippy, 131-test, rustdoc, feature-power-set, dependency audit, vet, machete,
coverage, and Python-test stages. Repository coverage was 97.02% and all 296
marked critical branch outcomes were covered.

The final standard semver stage is currently blocked by an upstream-resolution
condition that also affects unchanged `origin/main`: the workspace locks
`takecell==0.1.1`, but `cargo-semver-checks` creates an unlocked scratch package
and selects `takecell==0.1.2`, whose Rust 1.96 requirement exceeds the
repository's Rust 1.90 MSRV. The same semver comparison passes for all four
shared crates when run with Cargo's MSRV-aware resolver fallback. This spike
does not change the semver gate because that work is in the separately recorded
Step 2 follow-up batch.

## Recorded capabilities contract

The pinned release reports bearer authentication as required,
`runtime.mode = server_agent`, server-side tool execution, and no split
runtime. It advertises session resources, session chat and streaming, explicit
session continuity/key headers, skills read API, runs/steer/approval, and model
locking. It reports `admin_config_rw`, `jobs_admin`, `memory_write_api`, audio,
and realtime voice as false. The full response is committed rather than
summarized into an assumed client contract.

## Stop condition and unmeasured phases

D23 is blocked: the HTTP shape must be changed or replaced before conversation
code is written. Candidates for a new architectural decision include routing
through Hermes's native cached gateway path or adding an upstream-reviewed
HTTP agent-cache contract. This spike does not choose between them.

Per the required measurement order, the following were not executed after the
architecture gate failed:

- a two-profile multiplex gateway and actual Pi RSS/latency measurements;
- live or fake credential-isolation tests in both directions;
- concurrent profile activity and home/memory/skills/MCP/toolset isolation;
- the dedicated `hermes-gateway` systemd sandbox on Linux;
- a throwaway cross-user socket group/ACL transition;
- gym's real MCP server attachment;
- `hermes prompt-size` before/after the §6.3 toolsets.

Therefore D24 remains unevaluated. There is no basis in this PR to accept the
single multiplexed process or to select the documented per-bot-process
fallback. No Pi, provider, credential, socket-isolation, or prompt-size result
is claimed from the Apple host.

If D23 is revised while retaining this route, the first exact Pi rerun from the
`nswarm` checkout is:

```console
/opt/nswarm/hermes-spike/venv/bin/python scripts/hermes_gateway_spike.py \
  measure-reuse --source /opt/nswarm/hermes-spike/source --samples 30 \
  --output /tmp/hermes-http-reuse-pi.json
```

The source checkout must first pass `verify-source`. A separate reviewed
multiplex/isolation harness must then be added before any RSS, credential,
socket or prompt-size command is considered valid. Building that later-phase
harness now would violate the architecture stop condition.

## Scope held

No Telegram delivery, production credential, private profile state, botkit
conversation code, Step 2 follow-up, real gym port, Step 4 socket-mode change,
or v0 modification was made. D25 profile governance remains mandatory but was
not represented as measured runtime behavior because the profile phase was not
reached.
