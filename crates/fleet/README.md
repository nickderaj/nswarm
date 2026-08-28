# fleet

Validates `bots/*.toml` and renders deterministic, hardened systemd units and
per-bot environment files. It also provides a non-mutating, manifest-derived
plan across an explicit host root:

```console
fleet plan --all /path/to/nswarm /path/to/host-root /path/to/decrypted.env
```

The final argument is ephemeral plaintext produced by the operator's `age` or
`pass` decrypt step. The parser performs no expansion or ambient-environment
fallback. Unit drift is shown in full; environment drift is reported only as
`clean` or `replace (contents redacted)`, so plan output cannot disclose secret
values.

Non-Telegram bot units retain systemd's deny-all IP policy with an explicit
loopback exception for Hermes. Telegram-enabled units omit `IPAddressDeny` and
`IPAddressAllow`: systemd hostname allowlisting would not be a stable Telegram
policy, so egress filtering for those units remains an unimplemented
firewall/network-namespace adapter rather than a claimed property. Each bot is
also limited to its dedicated `<bot>-access` supplementary group for a future
socket ACL adapter. The repository tests only deterministic unit text; group
creation, peer membership, socket ownership/mode, live network containment,
and enforcement by a real systemd host remain unmeasured.

Host mutation, service restart, and smoke testing remain deliberately separate
until this plan contract is proven against a disposable host fixture.
