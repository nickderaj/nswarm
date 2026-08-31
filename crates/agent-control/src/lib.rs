//! Enforced control plane for research and coding agent jobs.

mod policy;
mod provisioner;
mod research;
mod store;
mod types;

pub use policy::{Capability, Role};
pub use provisioner::{
    CredentialBroker, CredentialLease, LocalWorktreeProvisioner, NoSecretBroker, ProvisionError,
    WorktreeProvisioner, WorktreeRequest,
};
pub use research::{
    ClaimConfidence, ClaimKind, ResearchClaim, ResearchReport, ResearchReportError, SourceAudit,
};
pub use store::{ControlStore, FindingDisposition, ReviewAssessment, StoreError};
pub use types::{
    ArtifactKind, BriefError, CredentialGrant, JobBrief, JobId, JobState, LeaseKind, NetworkMode,
    NetworkPolicy, PathPolicy, ProfileId, ResourceLimits, RiskClass, SessionId, Sha, UnitId,
    VerificationCommand,
};
