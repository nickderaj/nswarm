# nswarm

`nswarm` is a single-user, self-hosted fleet of private agents. The project is
being rebuilt in Rust around declarative service manifests, per-bot SQLite
databases, Unix-socket capability boundaries, and a separately sandboxed Hermes
agent gateway.

The v1 build is intentionally isolated from the frozen v0 `ultron` repository.
No private data, credentials, databases, market data, or owner-specific paths
belong in this repository.

## Current status

Step 1 is in progress: repository policy, `botkit`, `fleet`, and the agent-job
control plane. See [`docs/BUILD_STATUS.md`](docs/BUILD_STATUS.md) for verified
checkpoints and remaining external gates.

## Tenancy

Single-user operation is a product decision. A future web adapter will rely on
the host's private network boundary and the same explicit owner identity used by
the Telegram adapter; the schemas do not pretend to support multiple tenants.

## Development

Install the pinned Rust toolchain and the tools listed in
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md), then run:

```console
just ci
```

The repository is licensed under the MIT License.
