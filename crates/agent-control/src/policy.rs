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
    /// Complete role vocabulary used by generated policy validation.
    pub const ALL: [Self; 6] = [
        Self::Research,
        Self::Coordinator,
        Self::Coder,
        Self::VerifierReviewer,
        Self::Integrator,
        Self::Shipper,
    ];

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
const GYM_PROFILE_CAPABILITIES: &[Capability] = &[Capability::EvidenceWrite];

#[cfg(test)]
mod tests {
    use super::{Capability, GYM_PROFILE_CAPABILITIES, Role};

    #[test]
    fn eval_capability_corpus_enforces_role_boundaries() {
        let case: serde_json::Value = serde_json::from_str(include_str!(
            "../../../eval/corpus/capability-boundaries.json"
        ))
        .expect("capability eval corpus parses");
        let assertions = case["input"]["assertions"]
            .as_array()
            .expect("capability assertions are an array");
        assert_eq!(
            assertions.len() as u64,
            case["expected"]["assertion_count"]
                .as_u64()
                .expect("expected assertion count")
        );
        for assertion in assertions {
            let role: Role = serde_json::from_value(assertion["role"].clone())
                .expect("corpus role uses the production encoding");
            let capability: Capability = serde_json::from_value(assertion["capability"].clone())
                .expect("corpus capability uses the production encoding");
            let expected = assertion["allowed"]
                .as_bool()
                .expect("allowed is a boolean");
            assert_eq!(
                role.can(capability),
                expected,
                "unexpected grant for {role:?} and {capability:?}"
            );
        }
    }

    #[test]
    fn committed_role_capability_map_matches_production_authority() {
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../../profiles/role-capabilities.json"))
                .expect("role capability map parses");
        assert_eq!(document["schema_version"], 1);
        let roles = document["roles"]
            .as_object()
            .expect("role capability map contains roles");
        assert_eq!(roles.len(), Role::ALL.len() + 1);
        for role in Role::ALL {
            let recorded: Vec<Capability> = serde_json::from_value(
                roles
                    .get(role.as_str())
                    .unwrap_or_else(|| panic!("missing capability map for {role:?}"))
                    .clone(),
            )
            .expect("capability map uses production encodings");
            assert_eq!(
                recorded,
                role.capabilities(),
                "capability drift for {role:?}"
            );
        }
        let gym: Vec<Capability> = serde_json::from_value(roles["gym"].clone())
            .expect("gym capability map uses production encodings");
        assert_eq!(gym, GYM_PROFILE_CAPABILITIES);
    }

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
        for role in Role::ALL {
            assert_eq!(Role::from_name(role.as_str()), Some(role));
        }
        assert_eq!(Role::from_name("deploy"), None);
    }
}
