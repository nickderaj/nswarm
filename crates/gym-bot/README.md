# gym-bot

The Step 2 vertical slice for gym: a transport-neutral `/weight <kg>` command,
a type-only `teloxide` adapter, a single read-only `body_metrics` MCP tool over
a Unix socket, and the v0/v1 SQLite-state parity harness.

This is a spike, not a deployable Telegram bot. It performs no Telegram polling
or delivery and does not include Hermes.
