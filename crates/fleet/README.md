# fleet

Validates `bots/*.toml` and renders deterministic, hardened systemd units and
per-bot environment files. Host mutation and deployment are intentionally not
implemented until rendering and drift checks are proven hermetically.
