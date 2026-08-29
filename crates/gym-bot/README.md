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
or delivery and does not include Hermes. Duplicate update keys are retained only
for the lifetime of one `CommandService`; durable cross-restart idempotency is a
later storage-boundary decision and is not claimed here.
