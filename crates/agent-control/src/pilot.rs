//! Trusted serial scheduler adapter for the Step 5 one-coder pilot.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::{BriefError, ControlStore, JobBrief, JobState, LeaseKind, ProfileId, Role, StoreError};

/// Fixed structural actors for one serial coding unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoderPilotActors {
    /// Brief issuer and lease authority; cannot edit or merge.
    pub coordinator: ProfileId,
    /// The only writable worker profile.
    pub coder: ProfileId,
    /// Exact-SHA mechanical verifier.
    pub verifier: ProfileId,
    /// First independent adversarial reviewer.
    pub reviewer_one: ProfileId,
    /// Second independent adversarial reviewer.
    pub reviewer_two: ProfileId,
    /// Fresh composition owner.
    pub integrator: ProfileId,
    /// Exact-SHA protected-merge authority.
    pub shipper: ProfileId,
}

impl CoderPilotActors {
    fn for_brief(brief: &JobBrief) -> Result<Self, BriefError> {
        let suffix = format!("{}-{}", brief.job_id, brief.unit_id);
        Ok(Self {
            coordinator: ProfileId::new(format!("coordinator-{suffix}"))?,
            coder: ProfileId::new(format!("coder-{suffix}"))?,
            verifier: ProfileId::new(format!("verifier-{suffix}"))?,
            reviewer_one: ProfileId::new(format!("reviewer-one-{suffix}"))?,
            reviewer_two: ProfileId::new(format!("reviewer-two-{suffix}"))?,
            integrator: ProfileId::new(format!("integrator-{suffix}"))?,
            shipper: ProfileId::new(format!("shipper-{suffix}"))?,
        })
    }

    const fn roles(&self) -> [(&ProfileId, Role); 7] {
        [
            (&self.coordinator, Role::Coordinator),
            (&self.coder, Role::Coder),
            (&self.verifier, Role::VerifierReviewer),
            (&self.reviewer_one, Role::VerifierReviewer),
            (&self.reviewer_two, Role::VerifierReviewer),
            (&self.integrator, Role::Integrator),
            (&self.shipper, Role::Shipper),
        ]
    }
}

/// Leases established before the coder receives its first tool grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoderPilotLeases {
    /// Unique mutable Hermes profile-home lease.
    pub profile: i64,
    /// Minimal non-overlapping writable path leases derived from the brief.
    pub paths: Vec<i64>,
}

/// Prepared, grounded serial coder unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialCoderPilot {
    /// Immutable unit identity and role split.
    pub actors: CoderPilotActors,
    /// Durable leases active for the worker.
    pub leases: CoderPilotLeases,
}

impl SerialCoderPilot {
    /// Creates or exactly replays one serial coder control-plane assignment.
    ///
    /// The adapter registers fixed role identities, derives isolated profile
    /// homes beneath one scheduler-owned root, grants only the coder's profile
    /// and brief-writable paths, and enters `grounding`. A different live coder
    /// prevents another unit from starting.
    ///
    /// # Errors
    ///
    /// Returns [`PilotError`] for an invalid root, another live coder, invalid
    /// brief, profile or lease conflict, or durable store failure.
    pub fn prepare(
        store: &mut ControlStore,
        brief: &JobBrief,
        profile_root: &Path,
        lease_expires_at: i64,
        now: i64,
    ) -> Result<Self, PilotError> {
        brief.validate()?;
        if !is_normalized_absolute(profile_root) {
            return Err(PilotError::InvalidProfileRoot(profile_root.to_path_buf()));
        }
        let actors = CoderPilotActors::for_brief(brief)?;
        let active = store.live_coder_profiles()?;
        if let Some(other) = active.iter().find(|profile| **profile != actors.coder) {
            return Err(PilotError::AnotherCoderActive((*other).clone()));
        }

        store.create_job(brief, now)?;
        for (profile, role) in actors.roles() {
            store.register_profile(
                profile,
                &brief.job_id,
                &brief.unit_id,
                role,
                &profile_root.join(profile.as_str()),
                now,
            )?;
        }
        store.transition(
            &actors.coordinator,
            &brief.unit_id,
            JobState::Leased,
            &format!("pilot-leased:{}:{}", brief.job_id, brief.unit_id),
            now,
        )?;
        let profile = store.acquire_lease(
            &actors.coordinator,
            &actors.coder,
            &brief.job_id,
            &brief.unit_id,
            LeaseKind::Profile,
            actors.coder.as_str(),
            lease_expires_at,
            now,
        )?;
        let mut paths = Vec::new();
        for path in minimal_roots(&brief.paths.writable) {
            paths.push(store.acquire_lease(
                &actors.coordinator,
                &actors.coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Path,
                path.to_string_lossy().as_ref(),
                lease_expires_at,
                now,
            )?);
        }
        store.transition(
            &actors.coder,
            &brief.unit_id,
            JobState::Grounding,
            &format!("pilot-grounding:{}:{}", brief.job_id, brief.unit_id),
            now,
        )?;
        Ok(Self {
            actors,
            leases: CoderPilotLeases { profile, paths },
        })
    }
}

fn minimal_roots(paths: &[PathBuf]) -> Vec<&Path> {
    let mut roots = paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    roots.sort_by_key(|path| path.components().count());
    roots.into_iter().fold(Vec::new(), |mut minimal, path| {
        if !minimal.iter().any(|root| path.starts_with(root)) {
            minimal.push(path);
        }
        minimal
    })
}

fn is_normalized_absolute(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
}

/// Serial pilot bootstrap failure.
#[derive(Debug, Error)]
pub enum PilotError {
    /// Profile homes must be derived under a normalized absolute root.
    #[error("pilot profile root must be normalized and absolute: {path}", path = .0.display())]
    InvalidProfileRoot(PathBuf),
    /// Step 5 permits only one live coder profile.
    #[error("another serial coder pilot is active: {0}")]
    AnotherCoderActive(ProfileId),
    /// Immutable brief or generated profile identity failed validation.
    #[error("pilot brief validation error: {0}")]
    Brief(#[from] BriefError),
    /// Durable control-plane mutation failed.
    #[error("pilot control-plane error: {0}")]
    Store(#[from] StoreError),
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::{PilotError, SerialCoderPilot};
    use crate::{
        ControlStore, CredentialGrant, JobBrief, JobId, JobState, NetworkMode, NetworkPolicy,
        PathPolicy, ResourceLimits, RiskClass, Sha, StoreError, UnitId, VerificationCommand,
    };

    fn brief(job: &str, unit: &str) -> JobBrief {
        JobBrief {
            job_id: JobId::new(job).expect("job id"),
            unit_id: UnitId::new(unit).expect("unit id"),
            goal: "Implement one contained change.".to_owned(),
            repository: "https://example.invalid/nswarm.git".to_owned(),
            base_sha: Sha::new("a".repeat(40)).expect("base SHA"),
            paths: PathPolicy {
                readable: vec![PathBuf::from("crates/assigned")],
                writable: vec![
                    PathBuf::from("crates/assigned"),
                    PathBuf::from("crates/assigned/src"),
                ],
                forbidden: vec![PathBuf::from("crates/sibling")],
            },
            dependencies: Vec::new(),
            acceptance_criteria: vec!["focused test passes".to_owned()],
            verification_commands: vec![VerificationCommand {
                program: "cargo".to_owned(),
                arguments: vec!["test".to_owned()],
            }],
            risk_class: RiskClass::Medium,
            limits: ResourceLimits {
                wall_seconds: 600,
                memory_bytes: 1_000_000,
                disk_bytes: 1_000_000,
                process_count: 8,
                cost_microunits: 0,
            },
            network: NetworkPolicy {
                mode: NetworkMode::DenyAll,
                destinations: Vec::new(),
            },
            credential_grants: vec![CredentialGrant {
                credential_id: "pilot-push".to_owned(),
                methods: vec![format!("git:push:refs/heads/nswarm/{job}/{unit}")],
            }],
            report_schema: json!({
                "type": "object",
                "required": ["schema_version"],
                "properties": {"schema_version": {"type": "integer"}},
                "additionalProperties": true
            }),
            standing_policy_version: "v1".to_owned(),
        }
    }

    #[test]
    fn serial_coder_preparation_is_scoped_and_replayable() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief("pilot-job", "pilot-unit");
        let first = SerialCoderPilot::prepare(
            &mut store,
            &brief,
            Path::new("/tmp/nswarm-pilot-profiles"),
            100,
            1,
        )
        .expect("pilot prepared");
        assert_eq!(
            store.state(&brief.unit_id).expect("unit state"),
            JobState::Grounding
        );
        assert_eq!(first.leases.paths.len(), 1, "nested roots collapse");
        let replayed = SerialCoderPilot::prepare(
            &mut store,
            &brief,
            Path::new("/tmp/nswarm-pilot-profiles"),
            100,
            2,
        )
        .expect("pilot replayed");
        assert_eq!(replayed, first);
    }

    #[test]
    fn another_live_coder_and_unsafe_root_fail_closed() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let first = brief("pilot-one", "unit-one");
        SerialCoderPilot::prepare(
            &mut store,
            &first,
            Path::new("/tmp/nswarm-pilot-profiles"),
            100,
            1,
        )
        .expect("first pilot prepared");
        let second = brief("pilot-two", "unit-two");
        assert!(matches!(
            SerialCoderPilot::prepare(
                &mut store,
                &second,
                Path::new("/tmp/nswarm-pilot-profiles"),
                100,
                2,
            ),
            Err(PilotError::AnotherCoderActive(_))
        ));

        let mut empty = ControlStore::open_in_memory().expect("store opens");
        assert!(matches!(
            SerialCoderPilot::prepare(&mut empty, &second, Path::new("relative/profiles"), 100, 2,),
            Err(PilotError::InvalidProfileRoot(_))
        ));
        assert!(matches!(
            empty.state(&second.unit_id),
            Err(StoreError::UnknownUnit(_))
        ));
    }

    #[test]
    fn live_coder_gate_uses_the_canonical_role_encoding() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let first = brief("first-job", "first-unit");
        SerialCoderPilot::prepare(
            &mut store,
            &first,
            Path::new("/tmp/nswarm-pilot-profiles"),
            100,
            1,
        )
        .expect("first pilot prepared");

        assert_eq!(
            SerialCoderPilot::prepare(
                &mut store,
                &brief("second-job", "second-unit"),
                Path::new("/tmp/nswarm-pilot-profiles"),
                100,
                2,
            )
            .expect_err("canonical coder role must block a second pilot")
            .to_string(),
            "another serial coder pilot is active: coder-first-job-first-unit"
        );
    }

    #[test]
    fn eval_serial_pilot_corpus_enforces_single_writer() {
        let case: serde_json::Value =
            serde_json::from_str(include_str!("../../../eval/corpus/serial-pilot.json"))
                .expect("serial pilot corpus parses");
        let scenarios = case["input"]["scenarios"]
            .as_array()
            .expect("scenarios are an array");
        assert_eq!(case["expected"]["single_writer_enforced"], true);
        for scenario in scenarios {
            let expected = scenario["expected_outcome"]
                .as_str()
                .expect("expected outcome is text");
            let scenario = scenario["kind"].as_str().expect("scenario is text");
            let mut store = ControlStore::open_in_memory().expect("store opens");
            let first_brief = brief("eval-pilot-one", "eval-unit-one");
            match scenario {
                "exact-replay" | "nested-writable-collapse" => {
                    let first = SerialCoderPilot::prepare(
                        &mut store,
                        &first_brief,
                        Path::new("/tmp/nswarm-eval-pilot"),
                        100,
                        1,
                    )
                    .expect("pilot prepared");
                    if scenario == "exact-replay" {
                        let replayed = SerialCoderPilot::prepare(
                            &mut store,
                            &first_brief,
                            Path::new("/tmp/nswarm-eval-pilot"),
                            100,
                            2,
                        )
                        .expect("pilot replayed");
                        assert_eq!(first, replayed);
                        assert_eq!(expected, "identical-assignment");
                    } else {
                        assert_eq!(first.leases.paths.len(), 1);
                        assert_eq!(expected, "one-path-lease");
                    }
                }
                "concurrent-coder-refused" => {
                    SerialCoderPilot::prepare(
                        &mut store,
                        &first_brief,
                        Path::new("/tmp/nswarm-eval-pilot"),
                        100,
                        1,
                    )
                    .expect("first pilot prepared");
                    assert!(matches!(
                        SerialCoderPilot::prepare(
                            &mut store,
                            &brief("eval-pilot-two", "eval-unit-two"),
                            Path::new("/tmp/nswarm-eval-pilot"),
                            100,
                            2,
                        ),
                        Err(PilotError::AnotherCoderActive(_))
                    ));
                    assert_eq!(expected, "another-coder-active");
                }
                "unsafe-profile-root-refused" => {
                    assert!(matches!(
                        SerialCoderPilot::prepare(
                            &mut store,
                            &first_brief,
                            Path::new("relative/eval-pilot"),
                            100,
                            1,
                        ),
                        Err(PilotError::InvalidProfileRoot(_))
                    ));
                    assert_eq!(expected, "invalid-profile-root");
                }
                _ => panic!("unknown serial pilot scenario: {scenario}"),
            }
        }
    }
}
