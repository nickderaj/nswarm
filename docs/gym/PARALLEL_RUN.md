# Gym v1 disposable parallel run

The v0 gym remains running, untouched, and authoritative. The v1 trial uses a
new Telegram token supplied outside git and writes only to a verified copy of
`gym.db`. Trial writes are disposable. This is not a cutover procedure.

## Prepare the copy

Run this only when the operator has explicitly authorized access to the source
database. Never use the live source path as the destination.

```console
./scripts/copy_gym_database.sh /explicit/source/gym.db \
  /explicit/disposable-trial/gym.db \
  --metadata /explicit/disposable-trial/gym-copy-metadata.json
```

The script uses SQLite's online backup API, so committed WAL content is included
without copying transient `-wal` or `-shm` files. It refuses an existing
destination, source/destination aliases and SQLite-family aliases. Before
publishing the destination it requires schema version 5, `integrity_check=ok`,
and no foreign-key violations. Metadata contains only the destination filename,
size, SHA-256 digest, schema/integrity facts and a disposable-state marker.

The Step 2 processed-update sidecar is new v1 runtime state. Start it empty in
the disposable directory; do not copy or share v0's `processed_updates` table.

## Required operator inputs

- A new `GYM_BOT_TOKEN`, supplied only through the Fleet-rendered gym env file.
- The numeric `OWNER_TELEGRAM_ID`, supplied through the same env file.
- The explicit copied `gym.db` path and a distinct processed-update sidecar.
- Linux-created `gym-agent`, `boss-agent`, and `gym-access` identities.

Do not place tokens, `.env` files, private database paths, database contents, or
Telegram transcripts in git or command logs.

## Start boundary

The v1 process must be configured with the disposable directory as its only gym
data root. Refuse startup if the configured database path aliases the recorded
source path or if schema/integrity validation fails. D23/D24 remain unresolved,
so agent-dependent coaching is unavailable and no Hermes identity is authorized
for `gym-access`. Only deterministic commands and reviewed MCP tools may run.

## Linux verification still required

macOS cannot validate Linux users, groups or systemd sandbox behavior. On a
throwaway Linux host, then on the Pi, run the committed Fleet/socket verifier
before starting the trial. Do not claim production readiness until it proves:

```console
sudo scripts/verify_gym_socket_linux.sh /run/gym/mcp.sock gym-access
```

- `/run/gym` is not world-accessible;
- `/run/gym/mcp.sock` is group-owned by `gym-access` with mode `0660`;
- only `boss-agent` and the selected gateway identity, once D23/D24 approve one,
  can connect;
- unrelated service users cannot connect;
- restart recreates the same ownership and mode;
- socket clients receive no database path or Telegram credential.

## Stop and discard

Stop only the v1 unit. Preserve v0 unchanged. Archive sanitized validation
metadata if useful, then delete the disposable v1 database and sidecar through
the operator's normal retention process. Never merge trial writes into v0.
