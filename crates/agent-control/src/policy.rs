use serde::{Deserialize, Serialize};

/// Structurally granted operation in the control plane.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// Read the assigned repository scope.
    RepositoryRead,
    /// Write the assigned repository scope.
    RepositoryWrite,
    /// Query brief-approved external sources.
    NetworkRead,
    /// Append evidence and schema-validated reports.
    EvidenceWrite,
    /// Create and update only the assigned branch.
    BranchPush,
    /// Administer briefs and worker leases.
    Coordinate,
    /// Publish an exact-SHA verification verdict.
    Verify,
    /// Compose already verified units.
    Integrate,
    /// Merge one explicitly authorized exact SHA.
    Merge,
}

/// Explicit agent role; roles are never inferred from prompt prose.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// Read-only evidence gathering.
    Research,
    /// Brief decomposition and lease administration.
    Coordinator,
    /// Scoped code author.
    Coder,
    /// Fresh-checkout verifier and reviewer.
    VerifierReviewer,
    /// Single owner of integration topology.
    Integrator,
    /// Capability-limited protected-branch merger.
    Shipper,
}

impl Role {
    /// Stable database and generated-policy representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Research => "research",
            Self::Coordinator => "coordinator",
            Self::Coder => "coder",
            Self::VerifierReviewer => "verifier-reviewer",
            Self::Integrator => "integrator",
            Self::Shipper => "shipper",
        }
    }

    /// Parses the stable database and generated-policy representation.
    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "research" => Some(Self::Research),
            "coordinator" => Some(Self::Coordinator),
            "coder" => Some(Self::Coder),
            "verifier-reviewer" => Some(Self::VerifierReviewer),
            "integrator" => Some(Self::Integrator),
            "shipper" => Some(Self::Shipper),
            _ => None,
        }
    }

    /// Returns the complete immutable capability set for this role.
    #[must_use]
    pub const fn capabilities(self) -> &'static [Capability] {
        match self {
            Self::Research => &[
                Capability::RepositoryRead,
                Capability::NetworkRead,
                Capability::EvidenceWrite,
            ],
            Self::Coordinator => &[Capability::Coordinate, Capability::EvidenceWrite],
            Self::Coder => &[
                Capability::RepositoryRead,
                Capability::RepositoryWrite,
                Capability::EvidenceWrite,
                Capability::BranchPush,
            ],
            Self::VerifierReviewer => &[
                Capability::RepositoryRead,
                Capability::EvidenceWrite,
                Capability::Verify,
            ],
            Self::Integrator => &[
                Capability::RepositoryRead,
                Capability::RepositoryWrite,
                Capability::EvidenceWrite,
                Capability::Integrate,
            ],
            Self::Shipper => &[Capability::Merge],
        }
    }

    /// Tests one structural capability grant.
    #[must_use]
    pub fn can(self, capability: Capability) -> bool {
        self.capabilities().contains(&capability)
    }
}

#[cfg(test)]
mod tests {
    use super::{Capability, Role};

    #[test]
    fn research_is_physically_read_only() {
        assert!(Role::Research.can(Capability::RepositoryRead));
        assert!(!Role::Research.can(Capability::RepositoryWrite));
        assert!(!Role::Research.can(Capability::BranchPush));
    }

    #[test]
    fn coder_cannot_merge_or_deploy() {
        assert!(Role::Coder.can(Capability::RepositoryWrite));
        assert!(!Role::Coder.can(Capability::Merge));
        assert!(!Role::Coder.can(Capability::Integrate));
    }

    #[test]
    fn shipper_cannot_edit() {
        assert!(Role::Shipper.can(Capability::Merge));
        assert!(!Role::Shipper.can(Capability::RepositoryRead));
        assert!(!Role::Shipper.can(Capability::RepositoryWrite));
    }

    #[test]
    fn control_roles_receive_only_their_declared_authority() {
        assert_eq!(Role::Coordinator.as_str(), "coordinator");
        assert!(Role::Coordinator.can(Capability::Coordinate));
        assert!(!Role::Coordinator.can(Capability::RepositoryWrite));

        assert_eq!(Role::VerifierReviewer.as_str(), "verifier-reviewer");
        assert!(Role::VerifierReviewer.can(Capability::Verify));
        assert!(!Role::VerifierReviewer.can(Capability::Integrate));

        assert_eq!(Role::Integrator.as_str(), "integrator");
        assert!(Role::Integrator.can(Capability::Integrate));
        assert!(!Role::Integrator.can(Capability::Merge));
        assert_eq!(Role::Research.as_str(), "research");
        assert_eq!(Role::Coder.as_str(), "coder");
        assert_eq!(Role::Shipper.as_str(), "shipper");
        for role in [
            Role::Research,
            Role::Coordinator,
            Role::Coder,
            Role::VerifierReviewer,
            Role::Integrator,
            Role::Shipper,
        ] {
            assert_eq!(Role::from_name(role.as_str()), Some(role));
        }
        assert_eq!(Role::from_name("deploy"), None);
    }
}
