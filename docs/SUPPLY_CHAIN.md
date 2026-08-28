# Supply-chain exemption inventory

Generated 2026-08-28 from the locked Cargo graph, `supply-chain/config.toml`, and crates.io's exact-version API. The version link is the registry source; publisher is the account attached to that exact release. `not exposed` means the exact-version API returned no publishing account, not that publisher identity was inferred. Paths are shortest representative dependency paths; a crate can have additional consumers.

All 38 entries remain exemptions, not audits: 6 direct/security-sensitive, 18 transitive bootstrap, and 14 build/dev-only. No exemption was converted to an audit without source-review evidence.

## Locked inventory

| Crate | Version/source | Publisher | Kind | Use/path | Criterion | Review class |
|---|---|---|---|---|---|---|
| `bitflags` | [2.13.1](https://crates.io/crates/bitflags/2.13.1) | [@KodrAus](https://crates.io/users/KodrAus) | transitive-runtime | agent-control → rusqlite → bitflags | `safe-to-deploy` | P2 transitive bootstrap |
| `cc` | [1.4.4](https://crates.io/crates/cc/1.4.4) | not exposed | transitive-build | agent-control → rusqlite → libsqlite3-sys → cc | `safe-to-deploy` | P3 build/dev-only |
| `errno` | [0.3.14](https://crates.io/crates/errno/0.3.14) | [@sunfishcode](https://crates.io/users/sunfishcode) | transitive-dev | agent-control → tempfile → rustix → errno | `safe-to-run` | P3 build/dev-only |
| `fallible-iterator` | [0.3.0](https://crates.io/crates/fallible-iterator/0.3.0) | [@dpc](https://crates.io/users/dpc) | transitive-runtime | agent-control → rusqlite → fallible-iterator | `safe-to-deploy` | P2 transitive bootstrap |
| `fallible-streaming-iterator` | [0.1.9](https://crates.io/crates/fallible-streaming-iterator/0.1.9) | not exposed | transitive-runtime | agent-control → rusqlite → fallible-streaming-iterator | `safe-to-deploy` | P2 transitive bootstrap |
| `fastrand` | [2.5.0](https://crates.io/crates/fastrand/2.5.0) | [@taiki-e](https://crates.io/users/taiki-e) | transitive-dev | agent-control → tempfile → fastrand | `safe-to-run` | P3 build/dev-only |
| `find-msvc-tools` | [0.1.11](https://crates.io/crates/find-msvc-tools/0.1.11) | not exposed | transitive-build | agent-control → rusqlite → libsqlite3-sys → cc → find-msvc-tools | `safe-to-deploy` | P3 build/dev-only |
| `itoa` | [1.0.18](https://crates.io/crates/itoa/1.0.18) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → serde_json → itoa | `safe-to-deploy` | P2 transitive bootstrap |
| `libc` | [0.2.189](https://crates.io/crates/libc/0.2.189) | [@rust-lang-owner](https://crates.io/users/rust-lang-owner) | transitive-dev | agent-control → tempfile → rustix → libc | `safe-to-run` | P3 build/dev-only |
| `libsqlite3-sys` | [0.38.2](https://crates.io/crates/libsqlite3-sys/0.38.2) | [@gwenn](https://crates.io/users/gwenn) | transitive-runtime | agent-control → rusqlite → libsqlite3-sys | `safe-to-deploy` | P1 direct/security-sensitive |
| `linux-raw-sys` | [0.12.1](https://crates.io/crates/linux-raw-sys/0.12.1) | [@sunfishcode](https://crates.io/users/sunfishcode) | transitive-dev | agent-control → tempfile → rustix → linux-raw-sys | `safe-to-run` | P3 build/dev-only |
| `memchr` | [2.8.3](https://crates.io/crates/memchr/2.8.3) | [@BurntSushi](https://crates.io/users/BurntSushi) | transitive-runtime | agent-control → serde_json → memchr | `safe-to-deploy` | P2 transitive bootstrap |
| `once_cell` | [1.21.4](https://crates.io/crates/once_cell/1.21.4) | [@matklad](https://crates.io/users/matklad) | transitive-dev | agent-control → tempfile → once_cell | `safe-to-run` | P3 build/dev-only |
| `pkg-config` | [0.3.34](https://crates.io/crates/pkg-config/0.3.34) | [@sdroege](https://crates.io/users/sdroege) | transitive-build | agent-control → rusqlite → libsqlite3-sys → pkg-config | `safe-to-deploy` | P3 build/dev-only |
| `proc-macro2` | [1.0.107](https://crates.io/crates/proc-macro2/1.0.107) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → serde → serde_derive → proc-macro2 | `safe-to-deploy` | P2 transitive bootstrap |
| `quote` | [1.0.47](https://crates.io/crates/quote/1.0.47) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → serde → serde_derive → quote | `safe-to-deploy` | P2 transitive bootstrap |
| `rusqlite` | [0.40.2](https://crates.io/crates/rusqlite/0.40.2) | [@gwenn](https://crates.io/users/gwenn) | direct-runtime | agent-control → rusqlite | `safe-to-deploy` | P1 direct/security-sensitive |
| `rustix` | [1.1.4](https://crates.io/crates/rustix/1.1.4) | [@sunfishcode](https://crates.io/users/sunfishcode) | transitive-dev | agent-control → tempfile → rustix | `safe-to-run` | P3 build/dev-only |
| `serde` | [1.0.229](https://crates.io/crates/serde/1.0.229) | [@dtolnay](https://crates.io/users/dtolnay) | direct-runtime | agent-control → serde | `safe-to-deploy` | P1 direct/security-sensitive |
| `serde_core` | [1.0.229](https://crates.io/crates/serde_core/1.0.229) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → serde → serde_core | `safe-to-deploy` | P2 transitive bootstrap |
| `serde_derive` | [1.0.229](https://crates.io/crates/serde_derive/1.0.229) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → serde → serde_derive | `safe-to-deploy` | P2 transitive bootstrap |
| `serde_json` | [1.0.151](https://crates.io/crates/serde_json/1.0.151) | [@dtolnay](https://crates.io/users/dtolnay) | direct-runtime | agent-control → serde_json | `safe-to-deploy` | P1 direct/security-sensitive |
| `serde_spanned` | [1.1.1](https://crates.io/crates/serde_spanned/1.1.1) | [@epage](https://crates.io/users/epage) | transitive-runtime | fleet → toml → serde_spanned | `safe-to-deploy` | P2 transitive bootstrap |
| `shlex` | [2.0.1](https://crates.io/crates/shlex/2.0.1) | [@fenhl](https://crates.io/users/fenhl) | transitive-build | agent-control → rusqlite → libsqlite3-sys → cc → shlex | `safe-to-deploy` | P3 build/dev-only |
| `smallvec` | [1.15.2](https://crates.io/crates/smallvec/1.15.2) | [@emilio](https://crates.io/users/emilio) | transitive-runtime | agent-control → rusqlite → smallvec | `safe-to-deploy` | P2 transitive bootstrap |
| `syn` | [3.0.4](https://crates.io/crates/syn/3.0.4) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → serde → serde_derive → syn | `safe-to-deploy` | P2 transitive bootstrap |
| `tempfile` | [3.27.0](https://crates.io/crates/tempfile/3.27.0) | [@Stebalien](https://crates.io/users/Stebalien) | direct-dev | agent-control → tempfile | `safe-to-run` | P3 build/dev-only |
| `thiserror` | [2.0.20](https://crates.io/crates/thiserror/2.0.20) | [@dtolnay](https://crates.io/users/dtolnay) | direct-runtime | agent-control → thiserror | `safe-to-deploy` | P1 direct/security-sensitive |
| `thiserror-impl` | [2.0.20](https://crates.io/crates/thiserror-impl/2.0.20) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → thiserror → thiserror-impl | `safe-to-deploy` | P2 transitive bootstrap |
| `toml` | [1.1.4+spec-1.1.0](https://crates.io/crates/toml/1.1.4+spec-1.1.0) | [@epage](https://crates.io/users/epage) | direct-runtime | fleet → toml | `safe-to-deploy` | P1 direct/security-sensitive |
| `toml_datetime` | [1.1.1+spec-1.1.0](https://crates.io/crates/toml_datetime/1.1.1+spec-1.1.0) | [@epage](https://crates.io/users/epage) | transitive-runtime | fleet → toml → toml_datetime | `safe-to-deploy` | P2 transitive bootstrap |
| `toml_parser` | [1.1.3+spec-1.1.0](https://crates.io/crates/toml_parser/1.1.3+spec-1.1.0) | [@epage](https://crates.io/users/epage) | transitive-runtime | fleet → toml → toml_parser | `safe-to-deploy` | P2 transitive bootstrap |
| `unicode-ident` | [1.0.24](https://crates.io/crates/unicode-ident/1.0.24) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → serde → serde_derive → proc-macro2 → unicode-ident | `safe-to-deploy` | P2 transitive bootstrap |
| `vcpkg` | [0.2.15](https://crates.io/crates/vcpkg/0.2.15) | [@waych](https://crates.io/users/waych) | transitive-build | agent-control → rusqlite → libsqlite3-sys → vcpkg | `safe-to-deploy` | P3 build/dev-only |
| `windows-link` | [0.2.1](https://crates.io/crates/windows-link/0.2.1) | [@kennykerr](https://crates.io/users/kennykerr) | transitive-dev | agent-control → tempfile → windows-sys → windows-link | `safe-to-run` | P3 build/dev-only |
| `windows-sys` | [0.61.2](https://crates.io/crates/windows-sys/0.61.2) | [@kennykerr](https://crates.io/users/kennykerr) | transitive-dev | agent-control → tempfile → windows-sys | `safe-to-run` | P3 build/dev-only |
| `winnow` | [1.0.4](https://crates.io/crates/winnow/1.0.4) | [@epage](https://crates.io/users/epage) | transitive-runtime | fleet → toml → winnow | `safe-to-deploy` | P2 transitive bootstrap |
| `zmij` | [1.0.23](https://crates.io/crates/zmij/1.0.23) | [@dtolnay](https://crates.io/users/dtolnay) | transitive-runtime | agent-control → serde_json → zmij | `safe-to-deploy` | P2 transitive bootstrap |

## Disposition and assigned follow-up

- **P1 — repository maintainer:** review direct runtime and native SQLite boundary crates first. Record a cargo-vet audit only after examining the exact source/version and satisfying the named criterion; otherwise keep the exemption.

- **P2 — repository maintainer:** seek trustworthy upstream audit imports or perform exact-version source review after P1. These are explicit bootstrap debt, not evidence that the crates were audited locally.

- **P3 — repository maintainer:** batch build/dev-only review after deploy-path dependencies. Native build tooling still affects produced artifacts even when it does not execute in the deployed service.

`cargo vet check` remains mandatory in `just ci`; a newly resolved third-party version has neither an exemption nor an audit and therefore fails closed. Publisher identity is context for prioritization only and is never treated as proof of safety.
