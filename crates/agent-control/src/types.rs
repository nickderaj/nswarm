use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

macro_rules! identifier {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("Creates a validated `", stringify!($name), "`.")]
            ///
            /// # Errors
            ///
            /// Returns [`BriefError`] if the identifier is empty or not made of
            /// lowercase ASCII letters, digits, and `-`.
            pub fn new(value: impl Into<String>) -> Result<Self, BriefError> {
                let value = value.into();
                if value.is_empty()
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
                {
                    return Err(BriefError::InvalidIdentifier(value));
                }
                Ok(Self(value))
            }

            #[doc = concat!("Returns this `", stringify!($name), "` as text.")]
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identifier!(JobId, "Stable identifier for an immutable agent job.");
identifier!(
    UnitId,
    "Stable identifier for one independently leased job unit."
);
identifier!(
    ProfileId,
    "Stable identifier for one isolated agent profile."
);
identifier!(SessionId, "Stable identifier for one profile conversation.");

/// Full Git object id used by verification and merge authorization.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Sha(String);

impl Sha {
    /// Parses a full SHA-1 or SHA-256 Git object id.
    ///
    /// # Errors
    ///
    /// Returns [`BriefError`] for abbreviated, uppercase, or non-hex values.
    pub fn new(value: impl Into<String>) -> Result<Self, BriefError> {
        let value = value.into();
        if !matches!(value.len(), 40 | 64)
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(BriefError::InvalidSha(value));
        }
        Ok(Self(value))
    }

    /// Returns the full object id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for Sha {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Risk classification that determines review requirements.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskClass {
    /// Documentation or isolated low-impact work.
    Low,
    /// Application changes needing independent review.
    Medium,
    /// Security, migration, policy, or trading changes.
    High,
}

/// Filesystem scope included in an immutable brief.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathPolicy {
    /// Repository-relative paths visible to the worker.
    pub readable: Vec<PathBuf>,
    /// Repository-relative paths the worker may edit.
    pub writable: Vec<PathBuf>,
    /// Repository-relative paths explicitly denied.
    pub forbidden: Vec<PathBuf>,
}

impl PathPolicy {
    /// Checks whether a repository-relative path may be read.
    #[must_use]
    pub fn can_read(&self, path: &Path) -> bool {
        contains_path(&self.readable, path) && !contains_path(&self.forbidden, path)
    }

    /// Checks whether a repository-relative path may be written.
    #[must_use]
    pub fn can_write(&self, path: &Path) -> bool {
        contains_path(&self.writable, path) && !contains_path(&self.forbidden, path)
    }

    fn validate(&self) -> Result<(), BriefError> {
        if self.readable.is_empty() || self.writable.is_empty() || self.forbidden.is_empty() {
            return Err(BriefError::EmptyPathPolicy);
        }
        for path in self
            .readable
            .iter()
            .chain(&self.writable)
            .chain(&self.forbidden)
        {
            require_safe_relative(path)?;
        }
        for writable in &self.writable {
            if !contains_path(&self.readable, writable) {
                return Err(BriefError::WritableNotReadable(writable.clone()));
            }
            if overlaps_any(&self.forbidden, writable) {
                return Err(BriefError::WritableForbidden(writable.clone()));
            }
        }
        Ok(())
    }
}

/// Network policy for one worker.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkMode {
    /// No network destinations are granted.
    DenyAll,
    /// Only explicitly named hosts may be reached.
    AllowList,
}

/// Fail-closed egress policy included in a job brief.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkPolicy {
    /// Network policy mode.
    pub mode: NetworkMode,
    /// DNS names allowed when mode is [`NetworkMode::AllowList`].
    pub destinations: Vec<String>,
}

impl NetworkPolicy {
    fn validate(&self) -> Result<(), BriefError> {
        match self.mode {
            NetworkMode::DenyAll if !self.destinations.is_empty() => {
                Err(BriefError::DestinationsWithDenyAll)
            }
            NetworkMode::AllowList if self.destinations.is_empty() => {
                Err(BriefError::EmptyNetworkAllowList)
            }
            NetworkMode::AllowList
                if self.destinations.iter().any(|host| {
                    host.is_empty()
                        || host == "*"
                        || host.contains('/')
                        || host.contains(char::is_whitespace)
                }) =>
            {
                Err(BriefError::InvalidNetworkDestination)
            }
            _ => Ok(()),
        }
    }
}

/// Time and machine-resource limits for one unit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    /// Maximum wall-clock runtime.
    pub wall_seconds: u64,
    /// Maximum resident memory.
    pub memory_bytes: u64,
    /// Maximum writable disk consumption.
    pub disk_bytes: u64,
    /// Maximum child processes.
    pub process_count: u32,
    /// Optional maximum provider spend in millionths of the billing unit.
    pub cost_microunits: u64,
}

impl ResourceLimits {
    const fn validate(&self) -> Result<(), BriefError> {
        if self.wall_seconds == 0
            || self.memory_bytes == 0
            || self.disk_bytes == 0
            || self.process_count == 0
        {
            return Err(BriefError::ZeroResourceLimit);
        }
        Ok(())
    }
}

/// Method-scoped, job-bound credential request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialGrant {
    /// Opaque broker-owned credential identifier; never a secret value.
    pub credential_id: String,
    /// Exact allowed operations such as `git:push:refs/heads/nswarm/job/unit`.
    pub methods: Vec<String>,
}

impl CredentialGrant {
    fn validate(&self) -> Result<(), BriefError> {
        if self.credential_id.trim().is_empty()
            || self.methods.is_empty()
            || self
                .methods
                .iter()
                .any(|method| method.trim().is_empty() || method == "*")
        {
            return Err(BriefError::InvalidCredentialGrant);
        }
        Ok(())
    }
}

/// One exact command a verifier must execute without a shell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationCommand {
    /// Executable name or approved absolute path.
    pub program: String,
    /// Literal argument vector; shell interpolation is never implied.
    pub arguments: Vec<String>,
}

impl VerificationCommand {
    fn validate(&self) -> Result<(), BriefError> {
        if self.program.trim().is_empty()
            || self.program.contains(char::is_whitespace)
            || self
                .arguments
                .iter()
                .any(|argument| argument.contains('\0'))
        {
            return Err(BriefError::InvalidVerificationCommand);
        }
        Ok(())
    }
}

/// Machine-validated immutable job brief.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobBrief {
    /// Job owning this independently leased unit.
    pub job_id: JobId,
    /// Unit to which the brief applies.
    pub unit_id: UnitId,
    /// Concrete goal and done predicate.
    pub goal: String,
    /// Pinned repository URL.
    pub repository: String,
    /// Exact base revision.
    pub base_sha: Sha,
    /// Read, write, and deny scopes.
    pub paths: PathPolicy,
    /// Units that must finish first.
    pub dependencies: Vec<UnitId>,
    /// Independently observable acceptance criteria.
    pub acceptance_criteria: Vec<String>,
    /// Exact verification commands.
    pub verification_commands: Vec<VerificationCommand>,
    /// Review and containment risk.
    pub risk_class: RiskClass,
    /// Fail-closed resource limits.
    pub limits: ResourceLimits,
    /// Fail-closed egress policy.
    pub network: NetworkPolicy,
    /// Opaque, method-scoped credential requests.
    pub credential_grants: Vec<CredentialGrant>,
    /// JSON Schema that the worker report must satisfy.
    pub report_schema: Value,
    /// Version of the root-owned standing policy bundle.
    pub standing_policy_version: String,
}

impl JobBrief {
    /// Validates every required brief field and cross-field invariant.
    ///
    /// # Errors
    ///
    /// Returns [`BriefError`] rather than allowing a worker to infer a missing
    /// goal, scope, proof, resource, network, credential, or report contract.
    pub fn validate(&self) -> Result<(), BriefError> {
        if self.goal.trim().is_empty() {
            return Err(BriefError::EmptyGoal);
        }
        if !(self.repository.starts_with("https://") || self.repository.starts_with("file://"))
            || self.repository.contains(char::is_whitespace)
        {
            return Err(BriefError::InvalidRepository);
        }
        self.paths.validate()?;
        if self.dependencies.contains(&self.unit_id) {
            return Err(BriefError::SelfDependency);
        }
        if self.acceptance_criteria.is_empty()
            || self
                .acceptance_criteria
                .iter()
                .any(|criterion| criterion.trim().is_empty())
        {
            return Err(BriefError::EmptyAcceptanceCriteria);
        }
        if self.verification_commands.is_empty() {
            return Err(BriefError::EmptyVerificationCommands);
        }
        for command in &self.verification_commands {
            command.validate()?;
        }
        self.limits.validate()?;
        self.network.validate()?;
        for grant in &self.credential_grants {
            grant.validate()?;
        }
        if !matches!(self.report_schema, Value::Object(_)) {
            return Err(BriefError::InvalidReportSchema);
        }
        if self.standing_policy_version.trim().is_empty() {
            return Err(BriefError::EmptyPolicyVersion);
        }
        Ok(())
    }
}

/// Enforced lifecycle for one coding unit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    /// Brief exists but no worker holds it.
    Pending,
    /// A live worker owns the unit lease.
    Leased,
    /// Worker is inspecting the repository and brief.
    Grounding,
    /// Worker is editing within its lease.
    Implementing,
    /// Worker is running the brief's local proof.
    SelfVerifying,
    /// A committed exact SHA is ready for an independent checkout.
    CandidateReady,
    /// Independent proof is running against the candidate SHA.
    IndependentlyVerifying,
    /// Independent review and finding disposition are in progress.
    Reviewing,
    /// Candidate SHA has a current passing verdict.
    Verified,
    /// Integrator owns composition topology.
    Integrating,
    /// Integrated SHA has a current passing verdict.
    Integrated,
    /// A shipper is authorized for one exact SHA.
    MergeAuthorized,
    /// Protected-branch merge completed.
    Merged,
    /// Work cannot proceed until an explicit dependency changes.
    Blocked,
    /// Candidate needs a new author revision.
    FixRequired,
    /// A newer unit or candidate replaced this one.
    Superseded,
    /// Work was intentionally ended without acceptance.
    Abandoned,
    /// Late or policy-invalid output is isolated for reconciliation.
    Quarantined,
}

impl JobState {
    /// Returns whether the state machine allows the requested mechanical edge.
    /// Exact-SHA and evidence gates are additionally enforced by the store.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Pending | Self::FixRequired | Self::Blocked,
                Self::Leased
            ) | (Self::Leased, Self::Grounding)
                | (Self::Grounding, Self::Implementing)
                | (Self::Implementing, Self::SelfVerifying)
                | (Self::SelfVerifying, Self::CandidateReady)
                | (Self::CandidateReady, Self::IndependentlyVerifying)
                | (Self::IndependentlyVerifying, Self::Reviewing)
                | (Self::Reviewing, Self::Verified)
                | (Self::Verified, Self::Integrating)
                | (Self::Integrating, Self::Integrated)
                | (Self::Integrated, Self::MergeAuthorized)
                | (Self::MergeAuthorized, Self::Merged)
                | (
                    Self::CandidateReady
                        | Self::IndependentlyVerifying
                        | Self::Reviewing
                        | Self::Verified,
                    Self::FixRequired
                )
                | (
                    Self::Pending
                        | Self::Leased
                        | Self::Grounding
                        | Self::Implementing
                        | Self::SelfVerifying
                        | Self::CandidateReady
                        | Self::IndependentlyVerifying
                        | Self::Reviewing
                        | Self::Verified
                        | Self::Integrating,
                    Self::Blocked | Self::Abandoned | Self::Quarantined | Self::Superseded
                )
        )
    }

    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Leased => "leased",
            Self::Grounding => "grounding",
            Self::Implementing => "implementing",
            Self::SelfVerifying => "self-verifying",
            Self::CandidateReady => "candidate-ready",
            Self::IndependentlyVerifying => "independently-verifying",
            Self::Reviewing => "reviewing",
            Self::Verified => "verified",
            Self::Integrating => "integrating",
            Self::Integrated => "integrated",
            Self::MergeAuthorized => "merge-authorized",
            Self::Merged => "merged",
            Self::Blocked => "blocked",
            Self::FixRequired => "fix-required",
            Self::Superseded => "superseded",
            Self::Abandoned => "abandoned",
            Self::Quarantined => "quarantined",
        }
    }
}

impl TryFrom<&str> for JobState {
    type Error = BriefError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pending" => Ok(Self::Pending),
            "leased" => Ok(Self::Leased),
            "grounding" => Ok(Self::Grounding),
            "implementing" => Ok(Self::Implementing),
            "self-verifying" => Ok(Self::SelfVerifying),
            "candidate-ready" => Ok(Self::CandidateReady),
            "independently-verifying" => Ok(Self::IndependentlyVerifying),
            "reviewing" => Ok(Self::Reviewing),
            "verified" => Ok(Self::Verified),
            "integrating" => Ok(Self::Integrating),
            "integrated" => Ok(Self::Integrated),
            "merge-authorized" => Ok(Self::MergeAuthorized),
            "merged" => Ok(Self::Merged),
            "blocked" => Ok(Self::Blocked),
            "fix-required" => Ok(Self::FixRequired),
            "superseded" => Ok(Self::Superseded),
            "abandoned" => Ok(Self::Abandoned),
            "quarantined" => Ok(Self::Quarantined),
            _ => Err(BriefError::UnknownState(value.to_owned())),
        }
    }
}

/// Lease categories whose overlap rules differ.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LeaseKind {
    /// One writer owns a repository path tree.
    Path,
    /// Exactly one integrator owns stack topology.
    Topology,
    /// One worker owns a mutable profile home.
    Profile,
}

impl LeaseKind {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Topology => "topology",
            Self::Profile => "profile",
        }
    }
}

fn contains_path(roots: &[PathBuf], path: &Path) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

fn overlaps_any(roots: &[PathBuf], path: &Path) -> bool {
    roots
        .iter()
        .any(|root| path.starts_with(root) || root.starts_with(path))
}

fn require_safe_relative(path: &Path) -> Result<(), BriefError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(BriefError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

/// Immutable brief validation failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BriefError {
    /// Job and unit ids use a strict portable alphabet.
    #[error("invalid identifier: {0}")]
    InvalidIdentifier(String),
    /// Verification requires full object ids.
    #[error("invalid full Git SHA: {0}")]
    InvalidSha(String),
    /// The worker must receive an explicit goal.
    #[error("goal must not be empty")]
    EmptyGoal,
    /// Repository must be a pinned HTTPS or local fixture URL.
    #[error("repository must be an HTTPS or file URL without whitespace")]
    InvalidRepository,
    /// All three path classes are required.
    #[error("readable, writable, and forbidden paths must all be non-empty")]
    EmptyPathPolicy,
    /// Worker paths are repository-relative and cannot escape.
    #[error("unsafe repository-relative path: {path}", path = .0.display())]
    UnsafePath(PathBuf),
    /// Writing a path also requires read scope.
    #[error("writable path is not readable: {path}", path = .0.display())]
    WritableNotReadable(PathBuf),
    /// Denied and writable roots may not overlap.
    #[error("writable path overlaps forbidden path: {path}", path = .0.display())]
    WritableForbidden(PathBuf),
    /// A unit cannot wait on itself.
    #[error("unit cannot depend on itself")]
    SelfDependency,
    /// Acceptance criteria are a refuse-to-spawn field.
    #[error("acceptance criteria must be non-empty")]
    EmptyAcceptanceCriteria,
    /// Exact verification commands are a refuse-to-spawn field.
    #[error("verification commands must be non-empty")]
    EmptyVerificationCommands,
    /// Commands are literal argv arrays, not shell fragments.
    #[error("invalid verification command")]
    InvalidVerificationCommand,
    /// Resource limits fail closed.
    #[error("resource limits must be non-zero")]
    ZeroResourceLimit,
    /// Deny-all cannot carry confusing unused destinations.
    #[error("deny-all network policy cannot contain destinations")]
    DestinationsWithDenyAll,
    /// Allow-list mode needs at least one host.
    #[error("network allow-list must not be empty")]
    EmptyNetworkAllowList,
    /// Wildcard and path-like network destinations are invalid.
    #[error("invalid network destination")]
    InvalidNetworkDestination,
    /// Credential grants must be opaque and method-scoped.
    #[error("invalid credential grant")]
    InvalidCredentialGrant,
    /// Worker output must have a machine-checkable object schema.
    #[error("report schema must be a JSON object")]
    InvalidReportSchema,
    /// The standing policy pin is mandatory.
    #[error("standing policy version must not be empty")]
    EmptyPolicyVersion,
    /// Database state text must map to a declared enum variant.
    #[error("unknown job state: {0}")]
    UnknownState(String),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{JobState, PathPolicy, Sha};

    #[test]
    fn abbreviated_sha_is_rejected() {
        assert!(Sha::new("abc123").is_err());
    }

    #[test]
    fn sibling_path_is_not_visible() {
        let policy = PathPolicy {
            readable: vec!["crates/assigned".into()],
            writable: vec!["crates/assigned".into()],
            forbidden: vec!["crates/sibling".into()],
        };
        assert!(policy.can_write(Path::new("crates/assigned/src/lib.rs")));
        assert!(!policy.can_read(Path::new("crates/sibling/src/lib.rs")));
        assert!(!policy.can_write(Path::new("crates/other/src/lib.rs")));
    }

    #[test]
    fn prose_cannot_skip_verification_states() {
        assert!(!JobState::Implementing.can_transition_to(JobState::Verified));
        assert!(!JobState::CandidateReady.can_transition_to(JobState::Merged));
        assert!(!JobState::Verified.can_transition_to(JobState::Merged));
    }
}
