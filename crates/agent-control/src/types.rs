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
    // coverage-critical
    pub fn can_read(&self, path: &Path) -> bool {
        require_safe_relative(path).is_ok()
            && contains_path(&self.readable, path)
            && !contains_path(&self.forbidden, path)
    }

    /// Checks whether a repository-relative path may be written.
    #[must_use]
    // coverage-critical
    pub fn can_write(&self, path: &Path) -> bool {
        require_safe_relative(path).is_ok()
            && contains_path(&self.writable, path)
            && !contains_path(&self.forbidden, path)
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
    // coverage-critical
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
        if !validate_report_schema(&self.report_schema) {
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
                    Self::Integrated | Self::MergeAuthorized,
                    Self::FixRequired
                        | Self::Blocked
                        | Self::Abandoned
                        | Self::Quarantined
                        | Self::Superseded
                )
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

/// Repository evidence artifact classes accepted by the control plane.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    /// Focused or repository-wide test output.
    TestReport,
    /// Coverage summary or machine-readable coverage artifact.
    CoverageReport,
    /// Exact candidate diff or patch evidence.
    Diff,
    /// Structured execution log.
    Log,
    /// Research claim/source manifest.
    ClaimManifest,
    /// Durable worker handoff.
    Handoff,
}

impl ArtifactKind {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TestReport => "test-report",
            Self::CoverageReport => "coverage-report",
            Self::Diff => "diff",
            Self::Log => "log",
            Self::ClaimManifest => "claim-manifest",
            Self::Handoff => "handoff",
        }
    }
}

/// Validates the bounded JSON-Schema subset accepted for worker reports.
// coverage-critical
pub fn validate_report_schema(schema: &Value) -> bool {
    schema.as_object().is_some_and(|object| {
        object.get("type").and_then(Value::as_str) == Some("object")
            && validate_schema_node(schema, 0, true)
    })
}

/// Checks a report recursively against a previously validated schema.
// coverage-critical
pub fn report_matches_schema(schema: &Value, report: &Value) -> bool {
    matches_schema_node(schema, report, 0)
}

fn validate_schema_node(schema: &Value, depth: usize, root: bool) -> bool {
    if depth > 16 {
        return false;
    }
    let Some(object) = schema.as_object() else {
        return false;
    };
    let allowed = [
        "type",
        "required",
        "properties",
        "items",
        "additionalProperties",
    ];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return false;
    }
    match object.get("type").and_then(Value::as_str) {
        Some("object") => {
            if object.contains_key("items") {
                return false;
            }
            let Some(properties) = object.get("properties").and_then(Value::as_object) else {
                return false;
            };
            if !properties
                .values()
                .all(|property| validate_schema_node(property, depth + 1, false))
            {
                return false;
            }
            if object
                .get("additionalProperties")
                .is_some_and(|value| !value.is_boolean())
            {
                return false;
            }
            let required = object.get("required").and_then(Value::as_array);
            if root && required.is_none_or(Vec::is_empty) {
                return false;
            }
            let mut names = std::collections::BTreeSet::new();
            required.is_none_or(|fields| {
                fields.iter().all(|field| {
                    field.as_str().is_some_and(|name| {
                        !name.is_empty() && properties.contains_key(name) && names.insert(name)
                    })
                })
            })
        }
        Some("array") => {
            object
                .keys()
                .all(|key| matches!(key.as_str(), "type" | "items"))
                && object
                    .get("items")
                    .is_some_and(|items| validate_schema_node(items, depth + 1, false))
        }
        Some("string" | "boolean" | "integer" | "number" | "null") => object.len() == 1,
        _ => false,
    }
}

fn matches_schema_node(schema: &Value, value: &Value, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let Some(schema) = schema.as_object() else {
        return false;
    };
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let (Some(value), Some(properties)) = (
                value.as_object(),
                schema.get("properties").and_then(Value::as_object),
            ) else {
                return false;
            };
            let required_present =
                schema
                    .get("required")
                    .and_then(Value::as_array)
                    .is_none_or(|fields| {
                        fields.iter().all(|field| {
                            field.as_str().is_some_and(|name| value.contains_key(name))
                        })
                    });
            let known_fields_match = value.iter().all(|(name, field)| {
                properties
                    .get(name)
                    .is_none_or(|field_schema| matches_schema_node(field_schema, field, depth + 1))
            });
            let extras_allowed = schema
                .get("additionalProperties")
                .and_then(Value::as_bool)
                .unwrap_or(true)
                || value.keys().all(|name| properties.contains_key(name));
            required_present && known_fields_match && extras_allowed
        }
        Some("array") => value.as_array().is_some_and(|items| {
            schema.get("items").is_some_and(|item_schema| {
                items
                    .iter()
                    .all(|item| matches_schema_node(item_schema, item, depth + 1))
            })
        }),
        Some("string") => value.is_string(),
        Some("boolean") => value.is_boolean(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("number") => value.is_number(),
        Some("null") => value.is_null(),
        _ => false,
    }
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

// coverage-critical
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
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::{
        ArtifactKind, BriefError, CredentialGrant, JobBrief, JobId, JobState, LeaseKind,
        NetworkMode, NetworkPolicy, PathPolicy, ResourceLimits, RiskClass, Sha, UnitId,
        VerificationCommand, report_matches_schema, validate_report_schema,
    };

    fn valid_brief() -> JobBrief {
        JobBrief {
            job_id: JobId::new("job-1").expect("valid job id"),
            unit_id: UnitId::new("unit-1").expect("valid unit id"),
            goal: "implement the scoped unit".to_owned(),
            repository: "https://example.invalid/repository.git".to_owned(),
            base_sha: Sha::new("a".repeat(40)).expect("valid SHA"),
            paths: PathPolicy {
                readable: vec![PathBuf::from("crates/assigned")],
                writable: vec![PathBuf::from("crates/assigned")],
                forbidden: vec![PathBuf::from("secrets")],
            },
            dependencies: Vec::new(),
            acceptance_criteria: vec!["the focused tests pass".to_owned()],
            verification_commands: vec![VerificationCommand {
                program: "cargo".to_owned(),
                arguments: vec!["test".to_owned()],
            }],
            risk_class: RiskClass::Medium,
            limits: ResourceLimits {
                wall_seconds: 60,
                memory_bytes: 1_048_576,
                disk_bytes: 1_048_576,
                process_count: 8,
                cost_microunits: 0,
            },
            network: NetworkPolicy {
                mode: NetworkMode::DenyAll,
                destinations: Vec::new(),
            },
            credential_grants: vec![CredentialGrant {
                credential_id: "git-push-job-1".to_owned(),
                methods: vec!["git:push:refs/heads/nswarm/job-1/unit-1".to_owned()],
            }],
            report_schema: json!({
                "type": "object",
                "required": ["status"],
                "properties": {"status": {"type": "string"}},
                "additionalProperties": false
            }),
            standing_policy_version: "v1".to_owned(),
        }
    }

    #[test]
    fn abbreviated_sha_is_rejected() {
        assert!(Sha::new("abc123").is_err());
    }

    #[test]
    fn eval_path_corpus_enforces_containment() {
        let case: serde_json::Value =
            serde_json::from_str(include_str!("../../../eval/corpus/path-containment.json"))
                .expect("path eval corpus parses");
        let policy: PathPolicy = serde_json::from_value(case["input"]["policy"].clone())
            .expect("path policy uses the production schema");
        let probes = case["input"]["probes"]
            .as_array()
            .expect("path probes are an array");
        assert_eq!(
            probes.len() as u64,
            case["expected"]["probe_count"]
                .as_u64()
                .expect("expected probe count")
        );
        for probe in probes {
            let path = Path::new(probe["path"].as_str().expect("probe path is text"));
            let allowed = match probe["operation"].as_str().expect("operation is text") {
                "read" => policy.can_read(path),
                "write" => policy.can_write(path),
                operation => panic!("unknown path operation: {operation}"),
            };
            assert_eq!(
                allowed,
                probe["allowed"].as_bool().expect("allowed is a boolean"),
                "unexpected path decision for {}",
                path.display()
            );
        }
    }

    #[test]
    fn every_compound_policy_clause_is_independently_enforced() {
        for empty in ["readable", "writable", "forbidden"] {
            let mut brief = valid_brief();
            match empty {
                "readable" => brief.paths.readable.clear(),
                "writable" => brief.paths.writable.clear(),
                "forbidden" => brief.paths.forbidden.clear(),
                _ => unreachable!(),
            }
            assert_eq!(brief.validate(), Err(BriefError::EmptyPathPolicy));
        }
        for unsafe_path in ["", "/absolute", "crates/../sibling"] {
            let mut brief = valid_brief();
            brief.paths.readable = vec![PathBuf::from(unsafe_path)];
            assert!(matches!(brief.validate(), Err(BriefError::UnsafePath(_))));
        }

        for destination in ["", "*", "host/path", "two hosts"] {
            let mut brief = valid_brief();
            brief.network = NetworkPolicy {
                mode: NetworkMode::AllowList,
                destinations: vec![destination.to_owned()],
            };
            assert_eq!(brief.validate(), Err(BriefError::InvalidNetworkDestination));
        }
        let mut brief = valid_brief();
        brief.network = NetworkPolicy {
            mode: NetworkMode::AllowList,
            destinations: vec!["api.example.invalid".to_owned()],
        };
        assert!(brief.validate().is_ok());

        for zero_field in ["wall", "memory", "disk", "process"] {
            let mut brief = valid_brief();
            match zero_field {
                "wall" => brief.limits.wall_seconds = 0,
                "memory" => brief.limits.memory_bytes = 0,
                "disk" => brief.limits.disk_bytes = 0,
                "process" => brief.limits.process_count = 0,
                _ => unreachable!(),
            }
            assert_eq!(brief.validate(), Err(BriefError::ZeroResourceLimit));
        }

        for invalid_grant in ["id", "methods", "blank-method", "wildcard"] {
            let mut brief = valid_brief();
            match invalid_grant {
                "id" => brief.credential_grants[0].credential_id.clear(),
                "methods" => brief.credential_grants[0].methods.clear(),
                "blank-method" => brief.credential_grants[0].methods = vec![" ".to_owned()],
                "wildcard" => brief.credential_grants[0].methods = vec!["*".to_owned()],
                _ => unreachable!(),
            }
            assert_eq!(brief.validate(), Err(BriefError::InvalidCredentialGrant));
        }
    }

    #[test]
    fn eval_transition_policy_corpus_fails_closed() {
        let case: serde_json::Value =
            serde_json::from_str(include_str!("../../../eval/corpus/transition-policy.json"))
                .expect("transition eval corpus parses");
        let transitions = case["input"]["transitions"]
            .as_array()
            .expect("transitions are an array");
        assert_eq!(
            transitions.len() as u64,
            case["expected"]["transition_count"]
                .as_u64()
                .expect("expected transition count")
        );
        for transition in transitions {
            let source: JobState = serde_json::from_value(transition["from"].clone())
                .expect("source state uses the production encoding");
            let target: JobState = serde_json::from_value(transition["to"].clone())
                .expect("target state uses the production encoding");
            assert_eq!(
                source.can_transition_to(target),
                transition["allowed"]
                    .as_bool()
                    .expect("allowed is a boolean"),
                "unexpected transition decision for {source:?} -> {target:?}"
            );
        }
    }

    #[test]
    fn every_brief_boundary_fails_closed() {
        assert!(valid_brief().validate().is_ok());

        let mut brief = valid_brief();
        brief.goal = "  ".to_owned();
        assert_eq!(brief.validate(), Err(BriefError::EmptyGoal));

        let mut brief = valid_brief();
        brief.repository = "ssh://example.invalid/repository".to_owned();
        assert_eq!(brief.validate(), Err(BriefError::InvalidRepository));

        let mut brief = valid_brief();
        brief.repository = "file:///tmp/nswarm-fixture".to_owned();
        assert!(brief.validate().is_ok());

        let mut brief = valid_brief();
        brief.repository = "https://example.invalid/has whitespace".to_owned();
        assert_eq!(brief.validate(), Err(BriefError::InvalidRepository));

        let mut brief = valid_brief();
        brief.paths.readable.clear();
        assert_eq!(brief.validate(), Err(BriefError::EmptyPathPolicy));

        let mut brief = valid_brief();
        brief.paths.readable = vec![PathBuf::from("../escape")];
        assert!(matches!(brief.validate(), Err(BriefError::UnsafePath(_))));

        let mut brief = valid_brief();
        brief.paths.writable = vec![PathBuf::from("crates/other")];
        assert!(matches!(
            brief.validate(),
            Err(BriefError::WritableNotReadable(_))
        ));

        let mut brief = valid_brief();
        brief.paths.forbidden = vec![PathBuf::from("crates")];
        assert!(matches!(
            brief.validate(),
            Err(BriefError::WritableForbidden(_))
        ));

        let mut brief = valid_brief();
        brief.dependencies.push(brief.unit_id.clone());
        assert_eq!(brief.validate(), Err(BriefError::SelfDependency));

        let mut brief = valid_brief();
        brief.acceptance_criteria = vec![" ".to_owned()];
        assert_eq!(brief.validate(), Err(BriefError::EmptyAcceptanceCriteria));

        let mut brief = valid_brief();
        brief.acceptance_criteria.clear();
        assert_eq!(brief.validate(), Err(BriefError::EmptyAcceptanceCriteria));

        let mut brief = valid_brief();
        brief.verification_commands.clear();
        assert_eq!(brief.validate(), Err(BriefError::EmptyVerificationCommands));

        let mut brief = valid_brief();
        brief.verification_commands[0].program = "cargo test".to_owned();
        assert_eq!(
            brief.validate(),
            Err(BriefError::InvalidVerificationCommand)
        );

        let mut brief = valid_brief();
        brief.limits.wall_seconds = 0;
        assert_eq!(brief.validate(), Err(BriefError::ZeroResourceLimit));

        let mut brief = valid_brief();
        brief
            .network
            .destinations
            .push("example.invalid".to_owned());
        assert_eq!(brief.validate(), Err(BriefError::DestinationsWithDenyAll));

        let mut brief = valid_brief();
        brief.network.mode = NetworkMode::AllowList;
        assert_eq!(brief.validate(), Err(BriefError::EmptyNetworkAllowList));

        let mut brief = valid_brief();
        brief.network.mode = NetworkMode::AllowList;
        brief.network.destinations.push("*".to_owned());
        assert_eq!(brief.validate(), Err(BriefError::InvalidNetworkDestination));

        let mut brief = valid_brief();
        brief.credential_grants[0].methods = vec!["*".to_owned()];
        assert_eq!(brief.validate(), Err(BriefError::InvalidCredentialGrant));

        let mut brief = valid_brief();
        brief.report_schema = json!({"type": "array", "items": {"type": "string"}});
        assert_eq!(brief.validate(), Err(BriefError::InvalidReportSchema));

        let mut brief = valid_brief();
        brief.standing_policy_version = " ".to_owned();
        assert_eq!(brief.validate(), Err(BriefError::EmptyPolicyVersion));
    }

    #[test]
    fn report_schema_is_bounded_and_enforced() {
        let schema = valid_brief().report_schema;
        assert!(validate_report_schema(&schema));
        assert!(report_matches_schema(&schema, &json!({"status": "ok"})));
        assert!(!report_matches_schema(&schema, &json!({})));
        assert!(!report_matches_schema(
            &schema,
            &json!({"status": "ok", "unreviewed": true})
        ));
        assert!(!report_matches_schema(&schema, &json!({"status": 1})));
        assert!(!validate_report_schema(&json!({
            "type": "object",
            "required": [],
            "properties": {}
        })));
        assert!(!validate_report_schema(&json!({
            "type": "object",
            "required": ["missing"],
            "properties": {}
        })));
        assert!(!validate_report_schema(&json!({
            "type": "object",
            "required": ["status", "status"],
            "properties": {"status": {"type": "string"}}
        })));
    }

    #[test]
    fn report_schema_depth_and_primitive_contracts_are_exact() {
        let schema = json!({
            "type": "object",
            "required": ["boolean", "integer", "number", "null", "array"],
            "properties": {
                "boolean": {"type": "boolean"},
                "integer": {"type": "integer"},
                "number": {"type": "number"},
                "null": {"type": "null"},
                "array": {"type": "array", "items": {"type": "string"}}
            },
            "additionalProperties": false
        });
        assert!(validate_report_schema(&schema));
        assert!(report_matches_schema(
            &schema,
            &json!({
                "boolean": true,
                "integer": -1,
                "number": 1.5,
                "null": null,
                "array": ["value"]
            })
        ));
        for (field, wrong) in [
            ("boolean", json!("true")),
            ("integer", json!(1.5)),
            ("number", json!("1.5")),
            ("null", json!(false)),
            ("array", json!([1])),
        ] {
            let mut report = json!({
                "boolean": true,
                "integer": 1,
                "number": 1.5,
                "null": null,
                "array": ["value"]
            });
            report[field] = wrong;
            assert!(!report_matches_schema(&schema, &report), "accepted {field}");
        }

        let nested = |arrays: usize| {
            let mut node = json!({"type": "string"});
            for _ in 0..arrays {
                node = json!({"type": "array", "items": node});
            }
            json!({
                "type": "object",
                "required": ["value"],
                "properties": {"value": node},
                "additionalProperties": false
            })
        };
        let nested_value = |arrays: usize| {
            let mut value = json!("leaf");
            for _ in 0..arrays {
                value = json!([value]);
            }
            json!({"value": value})
        };
        let boundary = nested(15);
        assert!(validate_report_schema(&boundary));
        assert!(report_matches_schema(&boundary, &nested_value(15)));
        assert!(!validate_report_schema(&nested(16)));
        assert!(!report_matches_schema(&nested(16), &nested_value(16)));

        assert!(!validate_report_schema(&json!({
            "type": "object",
            "required": ["value"],
            "properties": {"value": {"type": "array", "items": {"type": "string"}, "properties": {}}}
        })));
    }

    #[test]
    fn stable_state_and_artifact_encodings_are_complete() {
        let states = [
            JobState::Pending,
            JobState::Leased,
            JobState::Grounding,
            JobState::Implementing,
            JobState::SelfVerifying,
            JobState::CandidateReady,
            JobState::IndependentlyVerifying,
            JobState::Reviewing,
            JobState::Verified,
            JobState::Integrating,
            JobState::Integrated,
            JobState::MergeAuthorized,
            JobState::Merged,
            JobState::Blocked,
            JobState::FixRequired,
            JobState::Superseded,
            JobState::Abandoned,
            JobState::Quarantined,
        ];
        for state in states {
            assert_eq!(JobState::try_from(state.as_str()), Ok(state));
        }
        assert!(matches!(
            JobState::try_from("invented"),
            Err(BriefError::UnknownState(_))
        ));
        assert_eq!(LeaseKind::Path.as_str(), "path");
        assert_eq!(LeaseKind::Topology.as_str(), "topology");
        assert_eq!(LeaseKind::Profile.as_str(), "profile");
        assert_eq!(ArtifactKind::TestReport.as_str(), "test-report");
        assert_eq!(ArtifactKind::CoverageReport.as_str(), "coverage-report");
        assert_eq!(ArtifactKind::Diff.as_str(), "diff");
        assert_eq!(ArtifactKind::Log.as_str(), "log");
        assert_eq!(ArtifactKind::ClaimManifest.as_str(), "claim-manifest");
        assert_eq!(ArtifactKind::Handoff.as_str(), "handoff");
    }
}
