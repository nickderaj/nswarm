# nswarm

`nswarm` is a single-user, self-hosted fleet of private agents. The project is
being rebuilt in Rust around declarative service manifests, per-bot SQLite
databases, Unix-socket capability boundaries, and a separately sandboxed Hermes
agent gateway.

The v1 build is intentionally isolated from the frozen v0 `ultron` repository.
No private data, credentials, databases, market data, or owner-specific paths
belong in this repository.

## Current status

Step 1 merged as `ac4bfd5f35a1aa2fbbf76ed46f84e7644ca7b049` in PR #1.
Step 2 merged as `5d3f7ef4cb449df3cd9a90d4742a651140c6f3d9` in PR #2.
Step 3 merged as `c8c1b69390f607be7f0ae895441b2141c74a3d93` in PR #3. The
pinned Hermes `v2026.8.19` HTTP session route constructs a fresh `AIAgent` for
every request, including repeated turns on one explicit session ID. D23 and
section 6.3 must therefore be revisited before any `botkit` conversation code
is written; D24 remains unevaluated and no fallback topology has been selected.
See [`docs/HERMES_SPIKE.md`](docs/HERMES_SPIKE.md) for the reproducible evidence
and [`docs/BUILD_STATUS.md`](docs/BUILD_STATUS.md) for verified checkpoints and
remaining gates.

The post-Step 3 cleanup retains both Chrono and Jiff after measuring the locked
graph: Chrono remains required by `rmcp`, `schemars`, and `teloxide-core`, so
rewriting the direct gym call sites would remove no resolved crate or duplicate.
The gym slice's explicit stored-timestamp policy is documented in its crate
README.

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
