# gym-bot

The Step 2 vertical slice for gym: a transport-neutral `/weight <kg>` command,
a type-only `teloxide` adapter, a single read-only `body_metrics` MCP tool over
a Unix socket, and the v0/v1 SQLite-state parity harness.

The spike refuses to bind outside an owner-only runtime directory. On Linux it
also sets and verifies socket mode `0600`; the private directory is the access
boundary on Unix platforms that do not honor socket-inode modes. Fleet remains
responsible for assigning the eventual service group and deliberately widening
directory/socket access only after group ownership is correct.

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
