# gym-bot

The Step 4 gym port builds on the Step 2 slice with transport-neutral strength,
cardio, body-metric, plan, preference, batch and Health services; recorded
Telegram/HTTP adapter contracts; the reviewed 13-tool MCP surface; and expanded
v0/v1 SQLite-state parity.

The runtime requires a non-world-accessible group-readable parent and sets mode
`0660` on `/run/gym/mcp.sock`. Fleet renders `RuntimeDirectoryMode=0750`,
`UMask=0007`, and the dedicated `gym-access` group. D23/D24 remain unresolved,
so `boss-agent` is the only authorized peer and no Hermes gateway identity is
granted yet.

This is a spike, not a deployable Telegram bot. It performs no Telegram polling
or delivery and does not include Hermes. Generic `(surface, external_id)` update
keys are persisted in a separate SQLite sidecar so restarts cannot replay a
command and the frozen v0 gym schema remains unchanged. The two databases do
not share a transaction, so backups and restores must keep `gym.db` and the
processed-update sidecar together.

The Step 2 parser intentionally implements only `/weight <kg>`. Unlike v0, a
bare `/weight` does not list history, trailing tokens are rejected, and Python's
underscore numeric syntax is not accepted. Those commands return the bounded
usage reply rather than widening this first vertical slice.

The MCP process is likewise a transport spike: an accept error terminates it,
connections and handshakes are not yet capped or timed out, and a stale socket
is not unlinked automatically. A supervised deployment must provide a fresh
private `RuntimeDirectory`; connection limiting, handshake timeouts, and
transient-accept retry remain pre-deployment work.

The `body_metrics` `limit` bounds response rows, not database work. Preserving
v0 timestamp text while ordering by actual instants requires materializing the
indexed candidate window before exact filtering, sorting, and truncation. The
maximum `days = 365` request therefore reads roughly one year of candidate rows;
that is acceptable for current body-metric volumes, but a normalized indexed
instant or a separate scan bound is required before substantially larger data
sets are supported.

Stored `body_metrics.date` values inside that indexed candidate window must be
RFC 3339 with an explicit offset. This includes current v1 writes and normal v0
aware-datetime writes, but excludes offset-free text that a legacy naive Python
`datetime` could have produced. One invalid selected timestamp fails the whole
tool call as a generic storage error: rows are never silently dropped and no
partial-result contract is implied. Values outside the candidate window are not
audited by a query.
