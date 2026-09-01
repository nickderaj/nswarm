# Hermes gateway architecture-gate report

Status: the local warm-agent assumption failed on 2026-08-30, but the separate
provider-cache gate passed on 2026-09-01. D23 now retains the stable HTTP
session API on that measured basis. D24 is unblocked but has not been run.

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

Persisted transcript/prompt continuity preserves the upstream provider's
byte-prefix cache in the live follow-up below. The first claimed benefit—avoiding
fixed local construction work—still does not exist on the HTTP route. The
separate provider benefit does: after the prime, the measured stable prefix is
served entirely from the provider cache and reduces repeated-turn modeled cost
by 91.02%.

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
| New explicit session | 30 | 0.347 ms | 0.390 ms | 0.411 ms | 0.584 ms | 0.654 ms |
| Repeated explicit session | 30 | 0.338 ms | 0.359 ms | 0.372 ms | 0.433 ms | 0.446 ms |

The separate global route prime was 90.796 ms. Across all 62 chat calls (route
prime, 30 new-session calls, repeated-session prime, and 30 repeated-session
calls), the agent factory ran 62 times. The 31 calls for the repeated session
created 31 distinct instances. The complete raw samples, environment,
capabilities response, source anchors, and excluded claims are in
[`../spikes/hermes/evidence/http-reuse.json`](../spikes/hermes/evidence/http-reuse.json).

These are two classes of route-overhead timing with a deterministic fake agent;
they are not cold/warm `AIAgent` timings. Actual construction latency,
growing-transcript reload cost, live-model latency, provider-side prompt-cache
latency, and Raspberry Pi latency remain unmeasured. The fixed two-message
history deliberately proves durable reload while keeping the lifecycle trial
deterministic; it does not measure the reload curve before compaction.

## Live provider-cache follow-up

[`../scripts/hermes_provider_cache_spike.py`](../scripts/hermes_provider_cache_spike.py)
measures the upstream half that the deterministic lifecycle probe cannot. It
first runs the exact Hermes source verifier. The pin now also covers the cache
planner, custom-provider cache capability, Anthropic usage extractor and
canonical cache-bucket pricing path. The source contract proves that later HTTP
turns restore the system-prompt bytes and that every fresh `AIAgent` reapplies
provider cache markers before the call.

The paid trial then sends four matched pairs to Surplus Intelligence's
Anthropic-compatible endpoint, pinned to the healthy native Anthropic provider
and `claude-haiku-4.5`. Each pair contains the same growing transcript and a
24,576-byte controlled system prompt. The cold session receives a fresh
same-length 32-byte nonce near the start of that prompt; the long-lived session
retains one byte-identical prompt for the run. Cold executes first in every
pair. Output is capped at eight tokens. Per-run and per-cold nonces, response
text and request identifiers are never persisted.

The harness refuses to run without `--i-understand-this-spends-money`, an
environment-only `SURPLUS_API_KEY`, a positive ceiling no greater than $3, the
exact source pin and a provider price quote. Before inference it conservatively
reserves one input token per request byte at the more expensive of uncached or
cache-write rates, plus the maximum output. The final run's hard ceiling was
$1.00 and its full planned worst case was $0.2884.

| Turn | Cold write | Cold read | Long-lived write | Long-lived read | Cold cost | Long-lived cost | Cold latency | Long-lived latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 6,437 | 0 | 6,435 | 0 | 8,099 µUSD | 8,096 µUSD | 40.819 s | 8.642 s |
| 2 | 6,434 | 0 | 0 | 6,435 | 8,112 µUSD | 713 µUSD | 4.171 s | 23.271 s |
| 3 | 6,437 | 0 | 0 | 6,435 | 8,133 µUSD | 730 µUSD | 40.843 s | 6.054 s |
| 4 | 6,434 | 0 | 0 | 6,435 | 8,146 µUSD | 747 µUSD | 24.152 s | 10.562 s |

Across all four turns, cold sessions wrote 25,742 cache tokens and read none;
the long-lived session wrote 6,435 on its prime and read 19,305 across the
three repeats. For repeats only, provider-usage cost fell from 24,391 to 2,190
micro-USD, saving 22,201 micro-USD or 91.02%. Median end-to-end latency was
32.486 seconds cold and 9.602 seconds long-lived. Cost is derived solely from
the provider's uncached/cache-read/cache-write/output usage buckets multiplied
by that provider's published bucket rates. It is not represented as the
marketplace wallet's settlement debit.

The committed aggregate-only result is
[`../spikes/hermes/evidence/provider-cache.json`](../spikes/hermes/evidence/provider-cache.json).
Its integrity checker derives every turn cost, aggregate, saving and decision,
checks the spend ceiling and prompt-coverage floor, rejects raw output or secret
material, and runs in local, PR and merge-queue CI. The live measurement is an
architecture experiment, not a Hermes pilot, bot session or deployment.

## Validation

The exact source verifier, eleven evidence-integrity and derivation checks,
repository policy check, formatting check, and diff hygiene pass locally. The
standard `just ci`
run passed its policy, profile, generated-file, Fleet, formatting, check, eval,
Clippy, 131-test, rustdoc, feature-power-set, dependency audit, vet, machete,
coverage, and Python-test stages. Repository coverage was 97.02% and all 296
marked critical branch outcomes were covered.

The previously red standard semver stage is repaired without changing the Rust
1.90 MSRV or weakening the comparison. `cargo-semver-checks` creates unlocked
scratch packages; the runner now explicitly uses Cargo's MSRV-aware resolver
fallback, selecting the newest compatible transitive versions instead of
`takecell==0.1.2`, which requires Rust 1.96. The formerly failing command passes
for all four shared crates locally without a shell-level override.

The committed Python checks exercise pin parsing, annotated-tag drift,
`sys.path` restoration, evidence integrity and summary derivation. Core CI does
not fetch the external Hermes repository, so the full source-byte/call-graph
verification remains an explicit execution gate against an already checked-out
source tree rather than a claim of continuous upstream monitoring.

## Recorded capabilities contract

The pinned release reports bearer authentication as required,
`runtime.mode = server_agent`, server-side tool execution, and no split
runtime. It advertises session resources, session chat and streaming, explicit
session continuity/key headers, skills read API, runs/steer/approval, and model
locking. It reports `admin_config_rw`, `jobs_admin`, `memory_write_api`, audio,
and realtime voice as false. The full response is committed rather than
summarized into an assumed client contract.

## Stop condition and unmeasured phases

D23 is revised: retain the stable HTTP session API. Fresh local construction is
a known cost and must be measured on the target Pi, but it does not destroy the
provider cache that dominates repeated input-token cost. The internal native
adapter and experimental Relay paths remain rejected because neither supplies
the reviewed synchronous external contract that `botkit` requires.

## Native-adapter follow-up

The pinned source was re-inspected for a route that keeps Hermes's native agent
cache while preserving `botkit` as the transport owner. The executable source
gate now pins and checks the relevant base-adapter and relay bytes. Its generated
result is [`../spikes/hermes/evidence/native-adapter.json`](../spikes/hermes/evidence/native-adapter.json).

| Candidate | Stable external contract | Warm agent | Required boundary | Result |
| --- | --- | --- | --- | --- |
| HTTP session API | yes | no | preserves `botkit` transport ownership | accepted by provider-cache measurement |
| Native platform adapter | no; Python internals | yes | Hermes owns Telegram; no synchronous `ask()` | rejected |
| Hermes Relay adapter | no; experimental | yes | needs an external connector and changing frame contract | rejected |

`BasePlatformAdapter.handle_message()` enters the cached gateway path, but it is
an internal Python method coupled to `MessageEvent`, `SessionSource`, runner
handler injection and private adapter lifecycle. Building an nswarm adapter
against it would turn internal upstream modules into an unreviewed API and would
not satisfy section 6.6's rule to couple only to declared contracts.

Relay is closer to the desired topology because a connector can own Telegram
while Hermes keeps the warm agent. The pinned source nevertheless labels its
adapter, transport and WebSocket protocol experimental, explicitly permits
changes without a deprecation cycle, and points to a separate connector repo for
the other half of the contract. Adopting it would replace one failed assumption
with an unpinned service and unstable wire format.

There is no stable local-agent-cache replacement to implement from the reviewed
pin, and the measured provider-cache result means one is not required to settle
D23. D24 may now evaluate the documented multiplexed gateway. If credential,
profile, socket, sandbox or Pi behavior fails that ordered trial, D24 still
falls back to one sandboxed gateway process and service user per bot.

Per the required measurement order, the following remain unexecuted until the
newly unblocked D24 harness is reviewed:

- a two-profile multiplex gateway and actual Pi RSS/latency measurements;
- live or fake credential-isolation tests in both directions;
- concurrent profile activity and home/memory/skills/MCP/toolset isolation;
- the dedicated `hermes-gateway` systemd sandbox on Linux;
- a throwaway cross-user socket group/ACL transition;
- gym's real MCP server attachment;
- `hermes prompt-size` before/after the §6.3 toolsets.

Therefore D24 remains unevaluated, but it is no longer blocked by D23. There is
still no basis in this PR to accept the single multiplexed process or to select
the documented per-bot-process fallback. No Pi, credential, socket-isolation,
or prompt-size result is claimed from the Apple host.

The first exact D24 Pi run from the `nswarm` checkout is:

```console
/opt/nswarm/hermes-spike/venv/bin/python scripts/hermes_gateway_spike.py \
  measure-reuse --source /opt/nswarm/hermes-spike/source --samples 30 \
  --output /tmp/hermes-http-reuse-pi.json
```

The source checkout must first pass `verify-source`. A separate reviewed
multiplex/isolation harness must then be added before any RSS, credential,
socket or prompt-size command is considered valid. D23 now permits that work;
this provider-cache change deliberately does not implement or run it.

## Scope held

No Telegram delivery, production credential, private profile state, botkit
conversation code, Step 2 follow-up, real gym port, Step 4 socket-mode change,
or v0 runtime/code modification was made. The design-only D23 revision is
reviewed separately in
[`nickderaj/ultron#1`](https://github.com/nickderaj/ultron/pull/1). D25 profile
governance remains mandatory but was not represented as measured runtime
behavior because the profile phase was not reached.
