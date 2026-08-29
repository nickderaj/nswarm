//! Declarative fleet manifests and deterministic systemd rendering.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

const SYSTEMD_UNIT_DIRECTORY: &str = "etc/systemd/system";
const ENVIRONMENT_DIRECTORY: &str = "etc/nswarm";

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
    /// Dedicated supplementary group used by the runtime's socket ACL adapter.
    pub socket_group: String,
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
    // coverage-critical
    pub fn validate(&self) -> Result<(), ManifestError> {
        validate_identifier("bot.name", &self.bot.name)?;
        validate_identifier("bot.crate", &self.bot.crate_name)?;
        validate_identifier("bot.user", &self.bot.user)?;
        validate_identifier("agent.profile", &self.agent.profile)?;
        validate_identifier("surface.socket_group", &self.surface.socket_group)?;
        if self.surface.socket_group != format!("{}-access", self.bot.name) {
            return Err(ManifestError::InvalidSocketGroup);
        }

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
            if path.starts_with(&self.bot.state)
                || self.bot.state.starts_with(path)
                || path.starts_with(&self.bot.data)
                || self.bot.data.starts_with(path)
            {
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
            format!("SupplementaryGroups={}", self.surface.socket_group),
            format!("ExecStart={}", executable.display()),
            format!("WorkingDirectory={}", self.bot.prefix.display()),
            format!("EnvironmentFile=/etc/nswarm/{}.env", self.bot.name),
            format!("RuntimeDirectory={}", self.bot.name),
            "RuntimeDirectoryMode=0750".to_owned(),
            "Restart=on-failure".to_owned(),
            "RestartSec=5s".to_owned(),
            "UMask=0077".to_owned(),
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
            "SystemCallFilter=@system-service".to_owned(),
            "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6".to_owned(),
        ];
        if !self.surface.telegram {
            lines.extend([
                "IPAddressDeny=any".to_owned(),
                "IPAddressAllow=localhost".to_owned(),
            ]);
        }
        lines.extend([
            format!("MemoryMax={}", self.sandbox.memory_max),
            format!("CPUQuota={}", self.sandbox.cpu_quota),
            format!(
                "ReadWritePaths={} {}",
                self.bot.state.display(),
                self.bot.data.display()
            ),
        ]);
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
    /// Returns [`ManifestError`] for a missing variable or a value that could
    /// alter systemd `EnvironmentFile` line boundaries through a newline, NUL,
    /// trailing continuation, or leading quote.
    // coverage-critical
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
            if value.contains(['\n', '\r', '\0'])
                || value.ends_with('\\')
                || value.starts_with('"')
                || value.starts_with('\'')
            {
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

    /// Returns the host-root-relative paths governed by this manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] if the manifest no longer validates.
    pub fn installed_paths(&self) -> Result<(PathBuf, PathBuf), ManifestError> {
        self.validate()?;
        Ok((
            PathBuf::from(SYSTEMD_UNIT_DIRECTORY).join(format!("{}.service", self.bot.name)),
            PathBuf::from(ENVIRONMENT_DIRECTORY).join(format!("{}.env", self.bot.name)),
        ))
    }

    /// Plans unit and environment changes without mutating the host or exposing
    /// secret values in plan output.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] if rendering fails.
    pub fn render_host_plan(
        &self,
        installed_unit: Option<&str>,
        installed_environment: Option<&str>,
        secrets: &BTreeMap<String, String>,
    ) -> Result<String, ManifestError> {
        let unit_plan = match installed_unit {
            Some(installed) => self.render_diff(installed)?,
            None => self.render_diff("")?,
        };
        let environment = self.render_environment(secrets)?;
        let environment_plan = if installed_environment == Some(environment.as_str()) {
            "clean"
        } else {
            "replace (contents redacted)"
        };
        Ok(format!(
            "bot: {}\nunit:\n{unit_plan}environment: {environment_plan}\n",
            self.bot.name
        ))
    }
}

/// Parses strict dotenv-shaped plaintext obtained from a secrets-store
/// decrypt operation.
///
/// Values are never interpolated, unescaped, logged, or inherited from the
/// ambient process environment.
///
/// # Errors
///
/// Returns [`ManifestError`] for malformed lines, duplicate names, or invalid
/// portable environment names.
// coverage-critical
pub fn parse_secret_source(source: &str) -> Result<BTreeMap<String, String>, ManifestError> {
    let mut values = BTreeMap::new();
    for (index, line) in source.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once('=')
            .ok_or(ManifestError::InvalidSecretSourceLine(index + 1))?;
        if !is_environment_name(name) {
            return Err(ManifestError::InvalidSecretName(name.to_owned()));
        }
        if value.contains(['\r', '\0']) {
            return Err(ManifestError::UnsafeSecretValue(name.to_owned()));
        }
        if values.insert(name.to_owned(), value.to_owned()).is_some() {
            return Err(ManifestError::DuplicateSecretName(name.to_owned()));
        }
    }
    Ok(values)
}

/// Discovers and parses every `bots/*.toml` manifest in deterministic order.
///
/// # Errors
///
/// Returns [`ManifestError`] rather than silently dropping an unreadable
/// directory entry or malformed manifest.
pub fn discover_manifests(
    repository_root: &Path,
) -> Result<Vec<(PathBuf, BotManifest)>, ManifestError> {
    let bot_directory = repository_root.join("bots");
    let mut paths = fs::read_dir(&bot_directory)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "toml")
    });
    paths.sort();
    if paths.is_empty() {
        return Err(ManifestError::NoManifests(bot_directory));
    }
    let manifests = paths
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path)?;
            let manifest = BotManifest::parse(&source)?;
            Ok((path, manifest))
        })
        .collect::<Result<Vec<_>, ManifestError>>()?;
    let mut names = BTreeSet::new();
    for (_, manifest) in &manifests {
        if !names.insert(&manifest.bot.name) {
            return Err(ManifestError::DuplicateBotName(manifest.bot.name.clone()));
        }
    }
    Ok(manifests)
}

/// Renders every discovered bot unit in deterministic manifest order.
///
/// # Errors
///
/// Returns [`ManifestError`] if inventory discovery or any manifest/render
/// contract fails.
pub fn render_repository_units(
    repository_root: &Path,
) -> Result<Vec<(String, String)>, ManifestError> {
    discover_manifests(repository_root)?
        .into_iter()
        .map(|(_, manifest)| {
            let name = manifest.bot.name.clone();
            Ok((name, manifest.render_unit()?))
        })
        .collect()
}

/// Plans every manifest against an explicit host root without mutation.
///
/// Environment drift is reported only as clean or redacted replacement; no
/// secret value enters the returned plan.
///
/// # Errors
///
/// Returns [`ManifestError`] for manifest, host-root, or secret-source errors.
pub fn plan_repository(
    repository_root: &Path,
    host_root: &Path,
    secrets: &BTreeMap<String, String>,
) -> Result<String, ManifestError> {
    if !host_root.is_absolute() {
        return Err(ManifestError::PathNotAbsolute {
            field: "host_root",
            path: host_root.to_path_buf(),
        });
    }
    validate_repository(repository_root)?;
    let mut output = String::new();
    for (_, manifest) in discover_manifests(repository_root)? {
        let (unit_path, environment_path) = manifest.installed_paths()?;
        let unit = read_optional_host_file(&host_root.join(unit_path))?;
        let environment = read_optional_host_file(&host_root.join(environment_path))?;
        output.push_str(&manifest.render_host_plan(
            unit.as_deref(),
            environment.as_deref(),
            secrets,
        )?);
    }
    Ok(output)
}

fn read_optional_host_file(path: &Path) -> Result<Option<String>, ManifestError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ManifestError::Io(error)),
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
    let manifests = discover_manifests(repository_root)?;
    let mut names = Vec::with_capacity(manifests.len());
    for (_, manifest) in manifests {
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
    // coverage-critical
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

// coverage-critical
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

// coverage-critical
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

// coverage-critical
fn is_model_credential(value: &str) -> bool {
    matches!(value, "OPENROUTER_API_KEY" | "XAI_API_KEY")
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
    /// Socket groups are dedicated per bot and cannot grant an existing host group.
    #[error("surface.socket_group must be <bot.name>-access")]
    InvalidSocketGroup,
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
    /// Decrypted secret sources use strict, comment-free `NAME=value` lines.
    #[error("invalid decrypted secret source line {0}")]
    InvalidSecretSourceLine(usize),
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
    /// Bot names own installed paths and therefore must be globally unique.
    #[error("duplicate bot name in manifest inventory: {0}")]
    DuplicateBotName(String),
    /// A manifest references source that is absent from the clean clone.
    #[error("manifest source is missing: {path}", path = .0.display())]
    MissingRepositorySource(PathBuf),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::{
        BotManifest, GatewayManifest, ManifestError, discover_manifests, parse_secret_source,
        plan_repository, render_repository_units, validate_repository,
    };

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
socket_group = "gym-access"
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
            "UMask=0077",
            "SystemCallFilter=@system-service",
            "SupplementaryGroups=gym-access",
            "ReadWritePaths=/var/lib/gym-agent /srv/nswarm/gym",
            "InaccessiblePaths=/srv/nswarm/tutor",
        ] {
            assert!(unit.contains(required), "missing {required}");
        }
        assert!(!unit.contains(concat!("/home/", "nick")));
        assert!(!unit.contains("IPAddressDeny="));
        assert!(!unit.contains("IPAddressAllow="));
    }

    #[test]
    fn telegram_and_non_telegram_egress_rendering_is_explicit() {
        let telegram = BotManifest::parse(MANIFEST)
            .expect("Telegram manifest validates")
            .render_unit()
            .expect("Telegram unit renders");
        assert!(!telegram.contains("IPAddressDeny="));
        assert!(!telegram.contains("IPAddressAllow="));

        let local_only =
            BotManifest::parse(&MANIFEST.replace("telegram = true", "telegram = false"))
                .expect("non-Telegram manifest validates")
                .render_unit()
                .expect("non-Telegram unit renders");
        assert!(local_only.contains("IPAddressDeny=any"));
        assert!(local_only.contains("IPAddressAllow=localhost"));
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
    fn every_manifest_validation_clause_is_independently_enforced() {
        let manifest = BotManifest::parse(MANIFEST).expect("manifest validates");

        for denied in [manifest.bot.state.clone(), manifest.bot.data.clone()] {
            let mut candidate = manifest.clone();
            candidate.sandbox.deny_paths = vec![denied.clone()];
            assert!(matches!(
                candidate.validate(),
                Err(ManifestError::DeniedWritableRoot(path)) if path == denied
            ));
        }
        for denied_parent in [
            manifest.bot.state.parent().expect("state parent"),
            manifest.bot.data.parent().expect("data parent"),
        ] {
            let mut candidate = manifest.clone();
            candidate.sandbox.deny_paths = vec![denied_parent.to_path_buf()];
            assert!(matches!(
                candidate.validate(),
                Err(ManifestError::DeniedWritableRoot(path)) if path == denied_parent
            ));
        }
        for resource in ["memory", "cpu"] {
            let mut candidate = manifest.clone();
            match resource {
                "memory" => candidate.sandbox.memory_max.clear(),
                "cpu" => candidate.sandbox.cpu_quota.clear(),
                _ => unreachable!(),
            }
            assert!(matches!(
                candidate.validate(),
                Err(ManifestError::EmptyResourceLimit)
            ));
        }
        for path in ["", "/absolute", "profiles/../sibling"] {
            let mut candidate = manifest.clone();
            candidate.agent.soul = PathBuf::from(path);
            assert!(matches!(
                candidate.validate(),
                Err(ManifestError::UnsafeRepositoryPath { .. })
            ));
        }
        for identifier in ["", "Uppercase", "has space"] {
            let mut candidate = manifest.clone();
            candidate.bot.name = identifier.to_owned();
            assert!(matches!(
                candidate.validate(),
                Err(ManifestError::InvalidIdentifier { .. })
            ));
        }
        for secret in ["", "1TOKEN", "lowercase", "BAD-NAME"] {
            let mut candidate = manifest.clone();
            candidate.secrets.allow = vec![secret.to_owned()];
            assert!(matches!(
                candidate.validate(),
                Err(ManifestError::InvalidSecretName(name)) if name == secret
            ));
        }
    }

    #[test]
    fn manifest_identity_and_collection_guards_are_independent() {
        let manifest = BotManifest::parse(MANIFEST).expect("manifest validates");
        let mut candidate = manifest.clone();
        candidate.bot.data = candidate.bot.state.clone();
        assert!(matches!(
            candidate.validate(),
            Err(ManifestError::OverlappingWritableRoots)
        ));
        let mut candidate = manifest.clone();
        candidate.agent.toolsets.clear();
        assert!(matches!(
            candidate.validate(),
            Err(ManifestError::EmptyToolsets)
        ));
        let mut candidate = manifest.clone();
        candidate.secrets.allow = vec!["MODEL_TOKEN".to_owned(), "MODEL_TOKEN".to_owned()];
        assert!(matches!(
            candidate.validate(),
            Err(ManifestError::DuplicateSecretName(name)) if name == "MODEL_TOKEN"
        ));
        let mut candidate = manifest;
        candidate.bot.name = "boss".to_owned();
        candidate.surface.socket_group = "boss-access".to_owned();
        candidate.surface.peers.clear();
        candidate
            .validate()
            .expect("boss has no required upstream peer");
    }

    #[test]
    fn socket_group_is_dedicated_and_rendered() {
        let manifest = BotManifest::parse(MANIFEST).expect("manifest validates");
        let unit = manifest.render_unit().expect("unit renders");
        assert!(unit.contains("SupplementaryGroups=gym-access"));

        for socket_group in ["boss-agent", "wheel", "gym-agent", ""] {
            let source = MANIFEST.replace(
                "socket_group = \"gym-access\"",
                &format!("socket_group = \"{socket_group}\""),
            );
            assert!(matches!(
                BotManifest::parse(&source),
                Err(ManifestError::InvalidSocketGroup | ManifestError::InvalidIdentifier { .. })
            ));
        }
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
        for secret in ["ERROR_BOT_TOKEN", "LOG_BOT_TOKEN", "GITHUB_TOKEN"] {
            let source = source
                .replace("OPENROUTER_API_KEY", secret)
                .replace(", \"TELEGRAM_BOT_TOKEN\"", "");
            assert!(matches!(
                GatewayManifest::parse(&source),
                Err(ManifestError::InvalidGatewaySecret(rejected)) if rejected == secret
            ));
        }

        for revision in ["main", "master", "release-*"] {
            let source = source
                .replace("v2026.8.19", revision)
                .replace(", \"TELEGRAM_BOT_TOKEN\"", "");
            assert!(matches!(
                GatewayManifest::parse(&source),
                Err(ManifestError::UnpinnedRevision)
            ));
        }
        let invalid_source = source
            .replace("v2026.8.19", "v1")
            .replace("OPENROUTER_API_KEY", "invalid-name")
            .replace(", \"TELEGRAM_BOT_TOKEN\"", "");
        assert!(matches!(
            GatewayManifest::parse(&invalid_source),
            Err(ManifestError::InvalidGatewaySecret(secret)) if secret == "invalid-name"
        ));
        let source = source
            .replace("OPENROUTER_API_KEY", "XAI_API_KEY")
            .replace(", \"TELEGRAM_BOT_TOKEN\"", "");
        GatewayManifest::parse(&source).expect("XAI is an explicitly allowed model credential");

        let wrong_user = source.replace("user = \"hermes-gateway\"", "user = \"model-gateway\"");
        assert!(matches!(
            GatewayManifest::parse(&wrong_user),
            Err(ManifestError::InvalidGatewayUser)
        ));
        let invalid_port = source.replace("port = 8642", "port = 0");
        assert!(matches!(
            GatewayManifest::parse(&invalid_port),
            Err(ManifestError::InvalidGatewayPort)
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

    #[test]
    fn decrypted_secret_source_is_strict_and_non_interpolating() {
        let values = parse_secret_source(
            "GYM_BOT_TOKEN=synthetic-gym\nOPENROUTER_API_KEY=$NOT_EXPANDED=value\n",
        )
        .expect("strict source parses");
        assert_eq!(values["OPENROUTER_API_KEY"], "$NOT_EXPANDED=value");
        assert!(matches!(
            parse_secret_source("OPENROUTER_API_KEY=one\nOPENROUTER_API_KEY=two\n"),
            Err(ManifestError::DuplicateSecretName(name)) if name == "OPENROUTER_API_KEY"
        ));
        assert!(matches!(
            parse_secret_source("export OPENROUTER_API_KEY=value\n"),
            Err(ManifestError::InvalidSecretName(_))
        ));
        assert!(matches!(
            parse_secret_source("VALID=value\nmissing-separator\n"),
            Err(ManifestError::InvalidSecretSourceLine(2))
        ));
        assert!(matches!(
            parse_secret_source("VALID=unsafe\rvalue\n"),
            Err(ManifestError::UnsafeSecretValue(name)) if name == "VALID"
        ));
        assert_eq!(
            parse_secret_source("\nVALID=value\n\n").expect("empty lines are ignored")["VALID"],
            "value"
        );
        let manifest = BotManifest::parse(MANIFEST).expect("manifest validates");
        for unsafe_value in [
            "continued\\",
            "\"unclosed-double-quote",
            "'unclosed-single-quote",
            "embedded\nnewline",
            "embedded\rreturn",
            "embedded\0nul",
        ] {
            let values = BTreeMap::from([
                ("GYM_BOT_TOKEN".to_owned(), unsafe_value.to_owned()),
                ("OPENROUTER_API_KEY".to_owned(), "safe".to_owned()),
            ]);
            assert!(matches!(
                manifest.render_environment(&values),
                Err(ManifestError::UnsafeSecretValue(name)) if name == "GYM_BOT_TOKEN"
            ));
        }
        let mut no_secrets = manifest;
        no_secrets.secrets.allow.clear();
        assert_eq!(
            no_secrets
                .render_environment(&BTreeMap::new())
                .expect("empty allowlist renders no environment"),
            ""
        );
    }

    #[test]
    fn repository_unit_rendering_cannot_omit_a_manifest() {
        let repository = TempDir::new().expect("temporary repository");
        let bots = repository.path().join("bots");
        fs::create_dir(&bots).expect("create bot inventory");
        fs::write(bots.join("gym.toml"), MANIFEST).expect("write gym manifest");
        fs::write(
            bots.join("coach.toml"),
            MANIFEST
                .replace("name = \"gym\"", "name = \"coach\"")
                .replace("user = \"gym-agent\"", "user = \"coach-agent\"")
                .replace("prefix = \"/opt/gym\"", "prefix = \"/opt/coach\"")
                .replace(
                    "state = \"/var/lib/gym-agent\"",
                    "state = \"/var/lib/coach-agent\"",
                )
                .replace("data = \"/srv/nswarm/gym\"", "data = \"/srv/nswarm/coach\"")
                .replace(
                    "socket = \"/run/gym/mcp.sock\"",
                    "socket = \"/run/coach/mcp.sock\"",
                )
                .replace(
                    "socket_group = \"gym-access\"",
                    "socket_group = \"coach-access\"",
                )
                .replace("profile = \"gym\"", "profile = \"coach\""),
        )
        .expect("write coach manifest");
        let rendered = render_repository_units(repository.path()).expect("render inventory");
        assert_eq!(
            rendered
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["coach", "gym"]
        );
        assert!(rendered[0].1.contains("Description=nswarm coach bot"));
        assert!(rendered[1].1.contains("Description=nswarm gym bot"));
    }

    #[test]
    fn duplicate_bot_names_cannot_collide_on_installed_paths() {
        let repository = TempDir::new().expect("temporary repository");
        let bots = repository.path().join("bots");
        fs::create_dir(&bots).expect("create bot inventory");
        fs::write(bots.join("one.toml"), MANIFEST).expect("write first manifest");
        fs::write(bots.join("two.toml"), MANIFEST).expect("write second manifest");
        assert!(matches!(
            discover_manifests(repository.path()),
            Err(ManifestError::DuplicateBotName(name)) if name == "gym"
        ));
    }

    #[test]
    fn repository_plan_compares_both_artifacts_without_secret_output() {
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root");
        let host = TempDir::new().expect("temporary host root");
        let synthetic_secret = "synthetic-provider-value";
        let secrets =
            BTreeMap::from([("OPENROUTER_API_KEY".to_owned(), synthetic_secret.to_owned())]);
        let first = plan_repository(repository, host.path(), &secrets).expect("plan renders");
        assert!(first.contains("environment: replace (contents redacted)"));
        assert!(!first.contains(synthetic_secret));

        let manifest = BotManifest::parse(
            &fs::read_to_string(repository.join("bots/research.toml"))
                .expect("read research manifest"),
        )
        .expect("manifest parses");
        let (unit_path, environment_path) = manifest.installed_paths().expect("installed paths");
        let unit_path = host.path().join(unit_path);
        let environment_path = host.path().join(environment_path);
        fs::create_dir_all(unit_path.parent().expect("unit parent")).expect("create unit parent");
        fs::create_dir_all(environment_path.parent().expect("environment parent"))
            .expect("create environment parent");
        fs::write(
            unit_path,
            manifest.render_unit().expect("render installed unit"),
        )
        .expect("write installed unit");
        fs::write(
            environment_path,
            manifest
                .render_environment(&secrets)
                .expect("render installed environment"),
        )
        .expect("write installed environment");

        let clean = plan_repository(repository, host.path(), &secrets).expect("clean plan");
        assert!(clean.contains("unit:\nclean\n"));
        assert!(clean.contains("environment: clean"));
        assert!(!clean.contains(synthetic_secret));
    }
}
