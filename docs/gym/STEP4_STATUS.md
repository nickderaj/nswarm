# Gym Step 4 implementation status

Status: deterministic repository work is implemented. Live parallel operation
and agent conversation behavior remain externally gated.

## Implemented

- machine-validated v0 behavior inventory covering 15 Telegram commands, 13
  reviewed MCP tools, schema, jobs, Health, exports, approval and startup;
- transport-neutral gym request/service types and recorded Telegram adaptation;
- owner-first durable generic update deduplication from Step 2;
- frozen schema-v5 strength, cardio, body metric, plan/rating, stated preference,
  batch and Health repositories/services;
- restart-safe durable batch deadlines, source-message deduplication, retry
  snapshots and concurrent-append-safe completion;
- strict one-MiB Health payload contract, replay deduplication, conservative
  manual-cardio reconciliation, splits/HR samples, and all-or-nothing writes;
- reviewed 13-tool MCP allow-list over an actual rmcp Unix socket, including only
  narrow preference/plan writes and no resources, prompts, sampling, raw SQL,
  filesystem, shell or arbitrary network tools;
- consistent SQLite online backup with source/destination family alias refusal,
  no overwrite, schema/integrity/FK validation and sanitized metadata;
- normalized SQLite-state parity for weight, strength, cardio and stated
  preference, plus Health transaction/replay tests and deliberate drift cases;
- Fleet-owned gym manifest and generated unit for `/run/gym/mcp.sock`, mode
  `0660`, `gym-access`, `RuntimeDirectoryMode=0750`, and `UMask=0007`;
- D25 repository-owned SOUL, skill, seeded memory and canonical profile with
  write approval, ledger-compatible skills, no background review/curator, and
  the minimal `mcp-gym`, clarify, memory, session-search and skills toolsets.

## D23 and D24

No later merged decision revises the Step 3 result. Pinned Hermes constructs a
fresh `AIAgent` for each HTTP chat request. D23 remains unresolved and D24 was
not measured. Therefore this port does not implement a Hermes HTTP client,
conversation core or `ask()` endpoint, does not claim warm-agent reuse, and
does not authorize any Hermes gateway identity in `gym-access`. Agent-dependent
free text, plan generation, batch extraction and reflection fail closed with an
explicit unavailable response.

Settlement requires a reviewed architecture decision selecting and measuring a
cached native gateway route or an upstream-reviewed HTTP agent-cache contract,
then the D24 multiplex/isolation, Pi RSS/latency, profile/credential/tool/socket
isolation, sandbox and prompt-size sequence recorded in `docs/HERMES_SPIKE.md`.

## External trial gates

- supply a new Telegram token only through `/etc/nswarm/gym.env`;
- explicitly authorize and provide a source database path, then create the
  disposable copy with `scripts/copy_gym_database.sh`;
- run `scripts/verify_gym_socket_linux.sh` first on throwaway Linux and then the
  Pi; macOS results do not prove Linux ownership, membership or systemd policy;
- execute live recorded commands against the new bot while v0 remains the
  untouched system of record; never merge trial writes or cut over in PR #5.

No production credential, owner database, message history, transcript or
private path was read or committed while producing this implementation.
