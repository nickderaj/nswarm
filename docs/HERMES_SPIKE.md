# Hermes gateway architecture-gate report

Status: the local warm-agent assumption failed on 2026-08-30. A direct-provider
control on 2026-09-01 proved that a byte-identical marked prefix preserves the
upstream cache, but Hermes was not in that request path. D23's end-to-end HTTP
continuity gate therefore remains open. D24's pinned upstream regression suite
passes; the nswarm runtime, target-Pi resources and Linux enforcement remain
open.

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

The pinned source asserts that persisted transcript/prompt continuity can
reproduce the marked bytes used by the provider call. The live follow-up below
separately proves the provider preserves a byte-identical marked prefix, but it
calls the provider directly rather than traversing Hermes. It therefore does
not prove that the Hermes HTTP route reproduces those bytes end to end. The
first claimed benefit—avoiding fixed local construction work—does not exist on
the HTTP route; the separate provider control is encouraging but cannot close
D23 by itself.

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
measures a direct-provider control for the upstream half that the deterministic
lifecycle probe cannot. Hermes is source-verified before the paid calls but is
not in their request path. The harness
first runs the exact Hermes source verifier. The pin now also covers the cache
planner, custom-provider cache capability, Anthropic usage extractor and
canonical cache-bucket pricing path. The pinned source inspection asserts that
later HTTP turns restore the persisted system prompt and that every fresh
`AIAgent` reapplies provider cache markers before the call; the direct-provider
control does not independently verify that end-to-end byte reproduction.

The paid control then sends four matched pairs to Surplus Intelligence's
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
$1.00 and its full planned worst case was $0.2884. A missing cache-write quote
fails closed instead of being priced as ordinary input. After each successful
request, an ignored aggregate-only partial checkpoint records completed-call
count and metered cost; no response content or request identifier is retained.

| Turn | Cold write | Cold read | Long-lived write | Long-lived read | Cold cost | Long-lived cost | Cold latency | Long-lived latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 6,437 | 0 | 6,435 | 0 | 8,099 µUSD | 8,096 µUSD | 40.819 s | 8.642 s |
| 2 | 6,434 | 0 | 0 | 6,435 | 8,112 µUSD | 713 µUSD | 4.171 s | 23.271 s |
| 3 | 6,437 | 0 | 0 | 6,435 | 8,133 µUSD | 730 µUSD | 40.843 s | 6.054 s |
| 4 | 6,434 | 0 | 0 | 6,435 | 8,146 µUSD | 747 µUSD | 24.152 s | 10.562 s |

Both classes carried the same ephemeral cache marker. Across all four turns,
the fresh-prefix control wrote 25,742 cache tokens and read none;
the long-lived session wrote 6,435 on its prime and read 19,305 across the
three repeats. For repeats only, provider-usage cost fell from 24,391 to 2,190
micro-USD, saving 22,201 micro-USD or 91.02% against cache-marked fresh
sessions, which pay the provider's cache-write premium. Repricing those same
fresh-session prompt tokens at the ordinary uncached-input rate gives a 19,563
micro-USD comparator and an 88.81% reduction. Cost is derived solely from the
provider's uncached/cache-read/cache-write/output usage buckets multiplied by
that provider's published bucket rates. It is not represented as the
marketplace wallet's settlement debit. The four fixed-order pairs are
uncontrolled endpoint-latency samples; their medians support no comparative
latency conclusion.

The committed aggregate-only result is
[`../spikes/hermes/evidence/provider-cache.json`](../spikes/hermes/evidence/provider-cache.json).
Its integrity checker derives every turn cost, aggregate, saving and decision,
checks the spend ceiling and prompt-coverage floor, rejects raw output or secret
material, accepts internally consistent favorable or adverse cache results, and
runs in local, PR and merge-queue CI. The live measurement is a direct-provider
architecture control, not a Hermes pilot, bot session or deployment.

## Validation

The exact source verifier, 49 focused Hermes tests, both aggregate-evidence
checkers, repository policy check, formatting check, and diff hygiene pass
locally. The standard `scripts/ci.sh` run passed its policy, profile,
generated-file, Fleet, formatting, check, eval, Clippy, 218-test, rustdoc,
feature-power-set, dependency audit, vet, machete, coverage, Python-test and
semver stages. Repository coverage was 97.33% and all 304 marked critical
branch outcomes were covered.

The previously red standard semver stage is repaired without changing the Rust
1.90 MSRV or weakening the comparison. `cargo-semver-checks` creates unlocked
scratch packages; the runner now explicitly uses Cargo's MSRV-aware resolver
fallback, selecting the newest compatible transitive versions instead of
`takecell==0.1.2`, which requires Rust 1.96. The formerly failing command passes
for all checked workspace crates locally without a shell-level override.

The committed Python checks exercise pin parsing, annotated-tag drift,
`sys.path` restoration, evidence integrity and summary derivation. Core CI does
not fetch the external Hermes repository, so the full source-byte/call-graph
verification remains an explicit execution gate against an already checked-out
source tree rather than a claim of continuous upstream monitoring.

The D24 local follow-up ran 27 additional pinned upstream files through
Hermes's canonical per-file-isolation runner with four workers and retries
disabled. All 288 tests passed, two optional-dependency cases skipped, and no
file needed a retry. The aggregate runner time was 4.9 seconds on the Apple
arm64 host. This is test-suite wall time, not gateway turn latency or a Pi
resource measurement.

## Recorded capabilities contract

The pinned release reports bearer authentication as required,
`runtime.mode = server_agent`, server-side tool execution, and no split
runtime. It advertises session resources, session chat and streaming, explicit
session continuity/key headers, skills read API, runs/steer/approval, and model
locking. It reports `admin_config_rw`, `jobs_admin`, `memory_write_api`, audio,
and realtime voice as false. The full response is committed rather than
summarized into an assumed client contract.

## Stop condition and unmeasured phases

D23 remains open: keep the stable HTTP session API as the candidate boundary,
but do not claim its provider-cache continuity until an end-to-end Hermes pair
confirms the provider usage buckets. Fresh local construction is a known cost
and must also be measured on the target Pi. The internal native adapter and
experimental Relay paths remain rejected because neither supplies the reviewed
synchronous external contract that `botkit` requires.

## Native-adapter follow-up

The pinned source was re-inspected for a route that keeps Hermes's native agent
cache while preserving `botkit` as the transport owner. The executable source
gate now pins and checks the relevant base-adapter and relay bytes. Its generated
result is [`../spikes/hermes/evidence/native-adapter.json`](../spikes/hermes/evidence/native-adapter.json).

| Candidate | Stable external contract | Warm agent | Required boundary | Result |
| --- | --- | --- | --- | --- |
| HTTP session API | yes | no | preserves `botkit` transport ownership | candidate; end-to-end cache continuity pending |
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
pin. D24's isolation evaluation is independently executable and need not wait
for D23's provider-cache result. If credential, profile, socket, sandbox or Pi
behavior fails that ordered trial, D24 falls back to one sandboxed gateway
process and service user per bot.

## D24 local multiplexing simulation

[`../scripts/hermes_multiplex_spike.py`](../scripts/hermes_multiplex_spike.py)
runs Hermes's canonical upstream regression suite against the same exact source pin. It
requires an explicit local-only acknowledgement, a clean tracked upstream
worktree and an isolated Python environment with the pinned test dependencies.
It caps the runner at four workers, disables retry-based green results, passes
only an explicit non-secret environment allowlist to the child, and stores no
raw test output. No provider credential is forwarded.

The selected upstream tests use temporary profile homes and cover credential
isolation, profile-prefixed HTTP routing and allowlists,
per-profile bearer authorization, provider/model secret scope, SOUL/config/
memory/skill scope, session-key namespaces, SQLite store scoping, concurrent
context propagation, background task scope, adapter lifecycle and pairing
stores. The result was 288 passed, 0 failed, 2 optional-dependency skips across
27 files, with zero flaky retries. The committed aggregate is
[`../spikes/hermes/evidence/multiplex-local.json`](../spikes/hermes/evidence/multiplex-local.json),
and its fail-closed checker runs in local, PR and merge-queue CI. Seven contract
groups are derived from explicit per-file pass/fail/skip counts. The two skips
belong only to a separately reported supplemental migration file; a skip in a
contract group would make that group incomplete rather than green. That file
validates optional tier-1 migration compatibility rather than the steady-state
multiplex credential boundary, so its missing-extra skips are reported as an
honest supplemental `incomplete` result instead of deciding the credential
contract.

This passes a pinned Hermes upstream regression baseline. It does not execute
nswarm's own two-profile gateway, so it does not pass D24's runtime isolation
gate or select a topology. Darwin cannot exercise the Linux service user,
systemd sandbox or Unix-socket
group/ACL transition, and its resource result cannot stand in for the target
Pi. The upstream tests use temporary profiles rather than attaching gym's real
MCP server, and no live pilot or gym parallel trial was run.

The following D24 acceptance work therefore remains target- or profile-specific:

- an nswarm-launched two-profile multiplex gateway with distinct profile homes,
  allowlists and bearer credentials;
- concurrent requests proving profile, session, memory and provider-secret
  isolation at the nswarm/Hermes boundary;
- actual Pi RSS/latency measurements for that two-profile process;
- the dedicated `hermes-gateway` systemd sandbox on Linux;
- a throwaway cross-user socket group/ACL transition;
- gym's real MCP server attachment;
- `hermes prompt-size` before/after the §6.3 toolsets.

Therefore D24 is locally de-risked but not settled. Its topology remains pending
the nswarm runtime trial; a failed isolation contract forces the per-bot-process
fallback. No Pi, Linux sandbox, socket-ACL, real-MCP, prompt-size or pilot result
is claimed from the Apple host.

The earlier stop condition said not to begin D24 in this change. The operator's
follow-up explicitly requested the local D24 simulation in the same draft PR,
so that stop condition is retracted for this evidence-only extension. It did
not authorize increased coder concurrency, the gym parallel trial or a live
pilot; none was run.

The first exact D24 Pi run from the `nswarm` checkout is:

```console
/opt/nswarm/hermes-spike/venv/bin/python scripts/hermes_gateway_spike.py \
  measure-reuse --source /opt/nswarm/hermes-spike/source --samples 30 \
  --output /tmp/hermes-http-reuse-pi.json
```

The source checkout must first pass `verify-source`. The reviewed local
multiplex harness has established an upstream regression baseline; the Pi command
does not by itself satisfy the remaining sandbox, socket or profile prompt-size
gates.

## Scope held

No Telegram delivery, production credential, private profile state, botkit
conversation code, Step 2 follow-up, real gym port, Step 4 socket-mode change,
or v0 runtime/code modification was made. The design-only D23 revision is
reviewed separately in
[`nickderaj/ultron#1`](https://github.com/nickderaj/ultron/pull/1). D25 profile
governance remains mandatory but was not represented as measured runtime
behavior because the profile phase was not reached.
