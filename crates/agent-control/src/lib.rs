//! Enforced control plane for research and coding agent jobs.

mod policy;
mod provisioner;
mod store;
mod types;

pub use policy::{Capability, Role};
pub use provisioner::{
    CredentialBroker, CredentialLease, LocalWorktreeProvisioner, NoSecretBroker, ProvisionError,
    WorktreeProvisioner, WorktreeRequest,
};
pub use store::{ControlStore, StoreError};
pub use types::{
    BriefError, CredentialGrant, JobBrief, JobId, JobState, LeaseKind, NetworkMode, NetworkPolicy,
    PathPolicy, ResourceLimits, RiskClass, Sha, UnitId, VerificationCommand,
};
