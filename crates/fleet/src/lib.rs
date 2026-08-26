//! Declarative fleet manifests and deterministic systemd rendering.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

/// Complete declarative description of one durable bot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BotManifest {
    /// Process identity and storage roots.
    pub bot: Bot,
    /// Front-end and socket surface.
    pub surface: Surface,
    /// Exact environment variable allow-list.
    pub secrets: Secrets,
    /// Root-owned Hermes profile material.
    pub agent: Agent,
    /// Resource limits and forbidden paths.
    pub sandbox: Sandbox,
}

/// Runtime supported by a fleet manifest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    /// A binary produced by this Cargo workspace.
    Rust,
    /// A separately packaged Python sidecar.
    Python,
}

/// Process identity and filesystem locations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bot {
    /// Stable bot name used by units and runtime directories.
    pub name: String,
    /// Cargo crate or Python package providing the executable.
    #[serde(rename = "crate")]
    pub crate_name: String,
    /// Language runtime used for installation.
    pub runtime: Runtime,
    /// Dedicated unprivileged service account.
    pub user: String,
    /// Root-owned installation prefix.
    pub prefix: PathBuf,
    /// Bot-writable runtime state root.
    pub state: PathBuf,
    /// Bot-writable domain data root.
    pub data: PathBuf,
}

/// Public and peer-facing surfaces for one bot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    /// Whether the Telegram adapter is enabled.
    pub telegram: bool,
    /// Unix-domain socket carrying MCP tools and `ask`.
    pub socket: PathBuf,
    /// Whether callers may request a full agent turn.
    pub ask: bool,
    /// Bot identities allowed to call this surface.
    pub peers: Vec<String>,
}

/// Exact environment-variable allow-list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Secrets {
    /// Variable names copied from the encrypted source during deployment.
    pub allow: Vec<String>,
}

/// Governed Hermes profile material.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    /// Multiplexed gateway profile name.
    pub profile: String,
    /// Repository-relative SOUL template.
    pub soul: PathBuf,
    /// Repository-relative skills directory.
    pub skills: PathBuf,
    /// Repository-relative seeded memory directory.
    pub memory: PathBuf,
    /// Reviewed Hermes toolsets granted to the profile.
    pub toolsets: Vec<String>,
    /// Must remain enabled so runtime writes are staged for review.
    pub write_approval: bool,
    /// Must remain disabled to prevent an unattended second model run.
    pub background_review: bool,
}

/// Resource and filesystem containment settings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Sandbox {
    /// systemd `MemoryMax` value.
    pub memory_max: String,
    /// systemd `CPUQuota` value.
    pub cpu_quota: String,
    /// Absolute paths made inaccessible to the service.
    pub deny_paths: Vec<PathBuf>,
}

impl BotManifest {
    /// Parses and validates a manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] for malformed TOML or a policy violation.
    pub fn parse(source: &str) -> Result<Self, ManifestError> {
        let manifest: Self = toml::from_str(source)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates manifest invariants that serde cannot express.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when identity, path, ACL, secret, governance,
    /// or resource policy is invalid.
    pub fn validate(&self) -> Result<(), ManifestError> {
        validate_identifier("bot.name", &self.bot.name)?;
        validate_identifier("bot.crate", &self.bot.crate_name)?;
        validate_identifier("bot.user", &self.bot.user)?;
        validate_identifier("agent.profile", &self.agent.profile)?;

        for (name, path) in [
            ("bot.prefix", self.bot.prefix.as_path()),
            ("bot.state", self.bot.state.as_path()),
            ("bot.data", self.bot.data.as_path()),
            ("surface.socket", self.surface.socket.as_path()),
        ] {
            require_absolute(name, path)?;
        }
        if self.bot.state == self.bot.data {
            return Err(ManifestError::OverlappingWritableRoots);
        }
        for path in &self.sandbox.deny_paths {
            require_absolute("sandbox.deny_paths", path)?;
            if path.starts_with(&self.bot.state) || path.starts_with(&self.bot.data) {
                return Err(ManifestError::DeniedWritableRoot(path.clone()));
            }
        }
        for (name, path) in [
            ("agent.soul", self.agent.soul.as_path()),
            ("agent.skills", self.agent.skills.as_path()),
            ("agent.memory", self.agent.memory.as_path()),
        ] {
            require_safe_relative(name, path)?;
        }
        if !self.agent.write_approval {
            return Err(ManifestError::WriteApprovalDisabled);
        }
        if self.agent.background_review {
            return Err(ManifestError::BackgroundReviewEnabled);
        }
        if self.agent.toolsets.is_empty() {
            return Err(ManifestError::EmptyToolsets);
        }
        if self.bot.name != "boss" && self.surface.peers != ["boss-agent"] {
            return Err(ManifestError::InvalidPeerTopology);
        }
        let mut names = BTreeSet::new();
        for name in &self.secrets.allow {
            if !is_environment_name(name) {
                return Err(ManifestError::InvalidSecretName(name.clone()));
            }
            if !names.insert(name) {
                return Err(ManifestError::DuplicateSecretName(name.clone()));
            }
        }
        if self.sandbox.memory_max.trim().is_empty() || self.sandbox.cpu_quota.trim().is_empty() {
            return Err(ManifestError::EmptyResourceLimit);
        }
        Ok(())
    }

    /// Renders the hardened systemd service unit deterministically.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] if the manifest no longer validates.
    pub fn render_unit(&self) -> Result<String, ManifestError> {
        self.validate()?;
        let executable = match self.bot.runtime {
            Runtime::Rust | Runtime::Python => {
                self.bot.prefix.join("bin").join(&self.bot.crate_name)
            }
        };
        let mut lines = vec![
            "[Unit]".to_owned(),
            format!("Description=nswarm {} bot", self.bot.name),
            "After=network-online.target hermes-gateway.service".to_owned(),
            "Wants=network-online.target".to_owned(),
            String::new(),
            "[Service]".to_owned(),
            "Type=simple".to_owned(),
            format!("User={}", self.bot.user),
            format!("Group={}", self.bot.user),
            format!("ExecStart={}", executable.display()),
            format!("WorkingDirectory={}", self.bot.prefix.display()),
            format!("EnvironmentFile=/etc/nswarm/{}.env", self.bot.name),
            format!("RuntimeDirectory={}", self.bot.name),
            "RuntimeDirectoryMode=0750".to_owned(),
            "Restart=on-failure".to_owned(),
            "RestartSec=5s".to_owned(),
            "NoNewPrivileges=true".to_owned(),
            "CapabilityBoundingSet=".to_owned(),
            "AmbientCapabilities=".to_owned(),
            "ProtectSystem=strict".to_owned(),
            "ProtectHome=true".to_owned(),
            "PrivateTmp=true".to_owned(),
            "PrivateDevices=true".to_owned(),
            "ProtectKernelTunables=true".to_owned(),
            "ProtectKernelModules=true".to_owned(),
            "ProtectKernelLogs=true".to_owned(),
            "ProtectControlGroups=true".to_owned(),
            "RestrictSUIDSGID=true".to_owned(),
            "LockPersonality=true".to_owned(),
            "MemoryDenyWriteExecute=true".to_owned(),
            "RestrictRealtime=true".to_owned(),
            "SystemCallArchitectures=native".to_owned(),
            "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6".to_owned(),
            format!("MemoryMax={}", self.sandbox.memory_max),
            format!("CPUQuota={}", self.sandbox.cpu_quota),
            format!(
                "ReadWritePaths={} {}",
                self.bot.state.display(),
                self.bot.data.display()
            ),
        ];
        if !self.sandbox.deny_paths.is_empty() {
            lines.push(format!(
                "InaccessiblePaths={}",
                self.sandbox
                    .deny_paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        lines.extend([
            String::new(),
            "[Install]".to_owned(),
            "WantedBy=multi-user.target".to_owned(),
        ]);
        Ok(format!("{}\n", lines.join("\n")))
    }

    /// Compares the deterministic unit with installed bytes.
    ///
    /// A clean host returns `clean`. Drift returns a complete replacement diff;
    /// callers never normalize or hide host edits.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] if the manifest no longer validates.
    pub fn render_diff(&self, installed: &str) -> Result<String, ManifestError> {
        let rendered = self.render_unit()?;
        if rendered == installed {
            return Ok("clean\n".to_owned());
        }
        let removed = installed
            .lines()
            .map(|line| format!("-{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let added = rendered
            .lines()
            .map(|line| format!("+{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!("--- installed\n+++ rendered\n{removed}\n{added}\n"))
    }

    /// Renders only the allow-listed environment variables in sorted order.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] for a missing variable or a value containing a
    /// newline or NUL byte.
    pub fn render_environment(
        &self,
        source: &BTreeMap<String, String>,
    ) -> Result<String, ManifestError> {
        self.validate()?;
        let mut rendered = Vec::with_capacity(self.secrets.allow.len());
        let mut names = self.secrets.allow.clone();
        names.sort_unstable();
        for name in names {
            let value = source
                .get(&name)
                .ok_or_else(|| ManifestError::MissingSecret(name.clone()))?;
            if value.contains(['\n', '\r', '\0']) {
                return Err(ManifestError::UnsafeSecretValue(name));
            }
            rendered.push(format!("{name}={value}"));
        }
        Ok(if rendered.is_empty() {
            String::new()
        } else {
            format!("{}\n", rendered.join("\n"))
        })
    }
}

/// Validates every `bots/*.toml` manifest and its repository-owned sources.
///
/// Enumeration is directory-derived so a new bot cannot be omitted from a
/// hand-maintained fleet list.
///
/// # Errors
///
/// Returns [`ManifestError`] for an unreadable directory, empty fleet, invalid
/// manifest, missing profile source, or missing crate/package scaffold.
pub fn validate_repository(repository_root: &Path) -> Result<Vec<String>, ManifestError> {
    let bot_directory = repository_root.join("bots");
    let mut paths = fs::read_dir(&bot_directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() {
        return Err(ManifestError::NoManifests(bot_directory));
    }
    let mut names = Vec::with_capacity(paths.len());
    for path in paths {
        let source = fs::read_to_string(&path)?;
        let manifest = BotManifest::parse(&source)?;
        for profile_source in [
            &manifest.agent.soul,
            &manifest.agent.skills,
            &manifest.agent.memory,
        ] {
            let resolved = repository_root.join(profile_source);
            if !resolved.exists() {
                return Err(ManifestError::MissingRepositorySource(resolved));
            }
        }
        let crate_manifest = repository_root
            .join("crates")
            .join(&manifest.bot.crate_name)
            .join("Cargo.toml");
        if !crate_manifest.is_file() {
            return Err(ManifestError::MissingRepositorySource(crate_manifest));
        }
        names.push(manifest.bot.name);
    }
    Ok(names)
}

/// Fleet-level Hermes gateway manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GatewayManifest {
    /// Dedicated unprivileged service user.
    pub user: String,
    /// Root-owned runtime install tree.
    pub prefix: PathBuf,
    /// Writable gateway runtime state.
    pub state: PathBuf,
    /// Loopback listener port.
    pub port: u16,
    /// Pinned upstream tag or full commit revision.
    pub revision: String,
    /// Explicit model/provider credential allow-list.
    pub secrets_allow: Vec<String>,
}

impl GatewayManifest {
    /// Parses and validates the gateway manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] for malformed TOML or a security policy
    /// violation.
    pub fn parse(source: &str) -> Result<Self, ManifestError> {
        let manifest: Self = toml::from_str(source)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates dedicated identity, paths, revision pin, and credential scope.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the gateway would be over-privileged.
    pub fn validate(&self) -> Result<(), ManifestError> {
        validate_identifier("user", &self.user)?;
        if self.user != "hermes-gateway" {
            return Err(ManifestError::InvalidGatewayUser);
        }
        require_absolute("prefix", &self.prefix)?;
        require_absolute("state", &self.state)?;
        if self.port == 0 {
            return Err(ManifestError::InvalidGatewayPort);
        }
        if self.revision == "main" || self.revision == "master" || self.revision.contains('*') {
            return Err(ManifestError::UnpinnedRevision);
        }
        for name in &self.secrets_allow {
            if !is_environment_name(name) || !is_model_credential(name) {
                return Err(ManifestError::InvalidGatewaySecret(name.clone()));
            }
        }
        Ok(())
    }

    /// Renders the loopback-only, unprivileged gateway service.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] if validation fails.
    pub fn render_unit(&self) -> Result<String, ManifestError> {
        self.validate()?;
        Ok(format!(
            "[Unit]\nDescription=nswarm Hermes gateway\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser={user}\nGroup={user}\nExecStart={prefix}/bin/hermes gateway --host 127.0.0.1 --port {port}\nWorkingDirectory={prefix}\nEnvironmentFile=/etc/nswarm/hermes-gateway.env\nRestart=on-failure\nRestartSec=5s\nNoNewPrivileges=true\nCapabilityBoundingSet=\nAmbientCapabilities=\nProtectSystem=strict\nProtectHome=true\nPrivateTmp=true\nPrivateDevices=true\nProtectKernelTunables=true\nProtectKernelModules=true\nProtectKernelLogs=true\nProtectControlGroups=true\nRestrictSUIDSGID=true\nLockPersonality=true\nMemoryDenyWriteExecute=true\nRestrictRealtime=true\nSystemCallArchitectures=native\nRestrictAddressFamilies=AF_UNIX AF_INET AF_INET6\nIPAddressDeny=any\nIPAddressAllow=localhost\nReadWritePaths={state}\n\n[Install]\nWantedBy=multi-user.target\n",
            user = self.user,
            prefix = self.prefix.display(),
            port = self.port,
            state = self.state.display(),
        ))
    }
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), ManifestError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ManifestError::InvalidIdentifier {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn require_absolute(field: &'static str, path: &Path) -> Result<(), ManifestError> {
    if !path.is_absolute() {
        return Err(ManifestError::PathNotAbsolute {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn require_safe_relative(field: &'static str, path: &Path) -> Result<(), ManifestError> {
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(ManifestError::UnsafeRepositoryPath {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn is_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && value.as_bytes()[0].is_ascii_uppercase()
}

fn is_model_credential(value: &str) -> bool {
    const FORBIDDEN_FRAGMENTS: [&str; 8] = [
        "TELEGRAM",
        "DATABASE",
        "TRADING",
        "DATABENTO",
        "WIREGUARD",
        "QBITTORRENT",
        "SSH",
        "OWNER",
    ];
    !FORBIDDEN_FRAGMENTS
        .iter()
        .any(|fragment| value.contains(fragment))
        && (value.ends_with("API_KEY") || value.ends_with("TOKEN"))
}

/// Manifest parsing and policy failures.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// Repository inventory or installed-unit read failed.
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML decoding failed before policy validation.
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    /// An identity is not portable to systemd and filesystem names.
    #[error("{field} has invalid identifier {value:?}")]
    InvalidIdentifier {
        /// Invalid manifest field.
        field: &'static str,
        /// Rejected value.
        value: String,
    },
    /// Host paths must be explicit and absolute.
    #[error("{field} must be absolute: {path}", path = .path.display())]
    PathNotAbsolute {
        /// Invalid manifest field.
        field: &'static str,
        /// Rejected path.
        path: PathBuf,
    },
    /// Profile sources must remain inside the repository.
    #[error("{field} must be a safe repository-relative path: {path}", path = .path.display())]
    UnsafeRepositoryPath {
        /// Invalid manifest field.
        field: &'static str,
        /// Rejected path.
        path: PathBuf,
    },
    /// State and domain data need distinct explicit roots.
    #[error("bot state and data roots must be distinct")]
    OverlappingWritableRoots,
    /// A deny path cannot shadow a writable root.
    #[error("deny path overlaps a writable root: {path}", path = .0.display())]
    DeniedWritableRoot(PathBuf),
    /// Runtime agent writes must always be staged.
    #[error("agent.write_approval must be true")]
    WriteApprovalDisabled,
    /// Autonomous post-turn writes are prohibited.
    #[error("agent.background_review must be false")]
    BackgroundReviewEnabled,
    /// A profile with no reviewed tools cannot perform useful work.
    #[error("agent.toolsets must not be empty")]
    EmptyToolsets,
    /// Worker peer ACLs must retain the star topology.
    #[error("worker peers must be exactly [\"boss-agent\"]")]
    InvalidPeerTopology,
    /// Environment variable names use a strict portable alphabet.
    #[error("invalid secret variable name: {0}")]
    InvalidSecretName(String),
    /// Duplicate allow-list entries obscure the rendered contract.
    #[error("duplicate secret variable name: {0}")]
    DuplicateSecretName(String),
    /// Resource limits may not silently fall back to unlimited.
    #[error("memory_max and cpu_quota must not be empty")]
    EmptyResourceLimit,
    /// Every allow-listed secret must exist at render time.
    #[error("missing allow-listed secret: {0}")]
    MissingSecret(String),
    /// systemd environment files are line-oriented.
    #[error("secret contains a forbidden control character: {0}")]
    UnsafeSecretValue(String),
    /// The gateway identity is fixed by the security contract.
    #[error("gateway user must be hermes-gateway")]
    InvalidGatewayUser,
    /// Port zero cannot be bound by the service contract.
    #[error("gateway port must be non-zero")]
    InvalidGatewayPort,
    /// Floating upstream revisions are prohibited.
    #[error("gateway revision must be a pinned tag or exact commit")]
    UnpinnedRevision,
    /// Gateway secrets may contain only model/provider credentials.
    #[error("gateway secret is outside the provider credential scope: {0}")]
    InvalidGatewaySecret(String),
    /// A fleet with no manifests cannot prove enumeration behaviour.
    #[error("no bot manifests found in {path}", path = .0.display())]
    NoManifests(PathBuf),
    /// A manifest references source that is absent from the clean clone.
    #[error("manifest source is missing: {path}", path = .0.display())]
    MissingRepositorySource(PathBuf),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{BotManifest, GatewayManifest, ManifestError, validate_repository};

    const MANIFEST: &str = r#"
[bot]
name = "gym"
crate = "gym-bot"
runtime = "rust"
user = "gym-agent"
prefix = "/opt/gym"
state = "/var/lib/gym-agent"
data = "/srv/nswarm/gym"

[surface]
telegram = true
socket = "/run/gym/mcp.sock"
ask = true
peers = ["boss-agent"]

[secrets]
allow = ["GYM_BOT_TOKEN", "OPENROUTER_API_KEY"]

[agent]
profile = "gym"
soul = "bots/gym/agent/SOUL.md"
skills = "bots/gym/agent/skills"
memory = "bots/gym/agent/memory"
toolsets = ["mcp-gym", "clarify"]
write_approval = true
background_review = false

[sandbox]
memory_max = "1G"
cpu_quota = "100%"
deny_paths = ["/srv/nswarm/tutor"]
"#;

    #[test]
    fn rendered_unit_has_mandatory_identity_and_sandbox() {
        let unit = BotManifest::parse(MANIFEST)
            .expect("manifest validates")
            .render_unit()
            .expect("render succeeds");
        for required in [
            "User=gym-agent",
            "EnvironmentFile=/etc/nswarm/gym.env",
            "NoNewPrivileges=true",
            "CapabilityBoundingSet=",
            "ProtectSystem=strict",
            "ReadWritePaths=/var/lib/gym-agent /srv/nswarm/gym",
            "InaccessiblePaths=/srv/nswarm/tutor",
        ] {
            assert!(unit.contains(required), "missing {required}");
        }
        assert!(!unit.contains(concat!("/home/", "nick")));
    }

    #[test]
    fn environment_contains_only_allow_list() {
        let manifest = BotManifest::parse(MANIFEST).expect("manifest validates");
        let source = BTreeMap::from([
            ("GYM_BOT_TOKEN".to_owned(), "synthetic-gym".to_owned()),
            (
                "OPENROUTER_API_KEY".to_owned(),
                "synthetic-model".to_owned(),
            ),
            ("TUTOR_BOT_TOKEN".to_owned(), "must-not-leak".to_owned()),
        ]);
        let rendered = manifest
            .render_environment(&source)
            .expect("allow list is complete");
        assert_eq!(
            rendered,
            "GYM_BOT_TOKEN=synthetic-gym\nOPENROUTER_API_KEY=synthetic-model\n"
        );
        assert!(!rendered.contains("TUTOR"));
    }

    #[test]
    fn governance_cannot_be_disabled() {
        let source = MANIFEST.replace("write_approval = true", "write_approval = false");
        assert!(matches!(
            BotManifest::parse(&source),
            Err(ManifestError::WriteApprovalDisabled)
        ));
        let source = MANIFEST.replace("background_review = false", "background_review = true");
        assert!(matches!(
            BotManifest::parse(&source),
            Err(ManifestError::BackgroundReviewEnabled)
        ));
    }

    #[test]
    fn worker_cannot_gain_a_sibling_peer() {
        let source = MANIFEST.replace(
            "peers = [\"boss-agent\"]",
            "peers = [\"boss-agent\", \"tutor-agent\"]",
        );
        assert!(matches!(
            BotManifest::parse(&source),
            Err(ManifestError::InvalidPeerTopology)
        ));
    }

    #[test]
    fn gateway_rejects_operational_credentials() {
        let source = r#"
user = "hermes-gateway"
prefix = "/opt/nswarm/hermes"
state = "/var/lib/hermes-gateway"
port = 8642
revision = "v2026.8.19"
secrets_allow = ["OPENROUTER_API_KEY", "TELEGRAM_BOT_TOKEN"]
"#;
        assert!(matches!(
            GatewayManifest::parse(source),
            Err(ManifestError::InvalidGatewaySecret(secret)) if secret == "TELEGRAM_BOT_TOKEN"
        ));
    }

    #[test]
    fn gateway_is_loopback_only() {
        let source = r#"
user = "hermes-gateway"
prefix = "/opt/nswarm/hermes"
state = "/var/lib/hermes-gateway"
port = 8642
revision = "v2026.8.19"
secrets_allow = ["OPENROUTER_API_KEY"]
"#;
        let unit = GatewayManifest::parse(source)
            .expect("gateway validates")
            .render_unit()
            .expect("gateway renders");
        assert!(unit.contains("--host 127.0.0.1"));
        assert!(unit.contains("IPAddressDeny=any"));
        assert!(unit.contains("IPAddressAllow=localhost"));
        assert!(unit.contains("User=hermes-gateway"));
    }

    #[test]
    fn render_diff_is_clean_or_reports_full_drift() {
        let manifest = BotManifest::parse(MANIFEST).expect("manifest validates");
        let rendered = manifest.render_unit().expect("unit renders");
        assert_eq!(manifest.render_diff(&rendered).expect("diff"), "clean\n");
        let diff = manifest
            .render_diff("[Unit]\nChanged=true\n")
            .expect("diff");
        assert!(diff.starts_with("--- installed\n+++ rendered\n"));
        assert!(diff.contains("-Changed=true"));
        assert!(diff.contains("+User=gym-agent"));
    }

    #[test]
    fn repository_validation_enumerates_manifests_without_a_list() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root");
        let names = validate_repository(root).expect("repository validates");
        assert_eq!(names, ["research"]);
    }
}
