use std::path::{Component, Path};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use thiserror::Error;

use crate::types::{BriefError, report_matches_schema};
use crate::{
    ArtifactKind, Capability, JobBrief, JobId, JobState, LeaseKind, ProfileId, RiskClass, Role,
    SessionId, Sha, UnitId,
};

const SCHEMA_VERSION: i64 = 9;

/// Reviewer assessment recorded against one exact candidate SHA.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewAssessment {
    /// Candidate must not advance until a new author revision resolves it.
    Blocking,
    /// Non-blocking concern requiring integrator disposition.
    Consider,
    /// Evidence was reviewed and no blocking issue was found.
    Noted,
    /// A previously raised concern was rejected with evidence.
    Dismissed,
}

impl ReviewAssessment {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Blocking => "blocking",
            Self::Consider => "consider",
            Self::Noted => "noted",
            Self::Dismissed => "dismissed",
        }
    }
}

/// One integrator-owned final disposition request.
pub struct FindingDisposition<'a> {
    /// Finding repository identifier.
    pub finding_id: i64,
    /// Final classification applied by the integrator.
    pub disposition: ReviewAssessment,
    /// Evidence supporting the disposition; redacted before persistence.
    pub rationale: &'a Value,
    /// Globally unique logical-operation key.
    pub idempotency_key: &'a str,
}

/// One verifier-attributed exact-SHA proof result.
pub struct VerificationVerdict<'a> {
    /// Live verifier profile publishing the result.
    pub verifier: &'a ProfileId,
    /// Exact candidate or integration revision that was tested.
    pub head_sha: &'a Sha,
    /// Machine result; any attributed failure blocks this exact SHA.
    pub passed: bool,
    /// Structured proof output, redacted before persistence.
    pub evidence: &'a Value,
    /// Globally unique logical-operation key.
    pub idempotency_key: &'a str,
}

/// Transactional `SQLite` repository for agent jobs and audit evidence.
pub struct ControlStore {
    connection: Connection,
}

impl ControlStore {
    /// Opens or creates a control-plane database and applies idempotent schema
    /// migrations.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `SQLite` cannot open or migrate the file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        let mut store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    /// Opens an isolated in-memory database for deterministic tests.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `SQLite` cannot initialize the schema.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()?;
        let mut store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    /// Applies the schema from empty or an already current database.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for an unsupported prior version or SQL failure.
    // coverage-critical
    fn migrate(&mut self) -> Result<(), StoreError> {
        self.connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchema(version));
        }
        let mut version = version;
        if version == 0 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(SCHEMA)?;
            transaction.pragma_update(None, "user_version", 1_i64)?;
            transaction.commit()?;
            version = 1;
        }
        if version == 1 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_2)?;
            transaction.pragma_update(None, "user_version", 2_i64)?;
            transaction.commit()?;
            version = 2;
        }
        if version == 2 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_3)?;
            transaction.pragma_update(None, "user_version", 3_i64)?;
            transaction.commit()?;
            version = 3;
        }
        if version == 3 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_4)?;
            transaction.pragma_update(None, "user_version", 4_i64)?;
            transaction.commit()?;
            version = 4;
        }
        if version == 4 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_5)?;
            transaction.pragma_update(None, "user_version", 5_i64)?;
            transaction.commit()?;
            version = 5;
        }
        if version == 5 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_6)?;
            transaction.pragma_update(None, "user_version", 6_i64)?;
            transaction.commit()?;
            version = 6;
        }
        if version == 6 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_7)?;
            transaction.pragma_update(None, "user_version", 7_i64)?;
            transaction.commit()?;
            version = 7;
        }
        if version == 7 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_8)?;
            transaction.pragma_update(None, "user_version", 8_i64)?;
            transaction.commit()?;
            version = 8;
        }
        if version == 8 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(MIGRATION_9)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        }
        Ok(())
    }

    /// Stores one validated immutable unit brief under its job.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when validation, serialization, or the atomic
    /// insert fails.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "trusted scheduler provisioning remains private until its command adapter is implemented"
        )
    )]
    // coverage-critical
    pub(crate) fn create_job(&mut self, brief: &JobBrief, now: i64) -> Result<(), StoreError> {
        brief.validate()?;
        let brief_json = serde_json::to_string(brief)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing_scope: Option<(String, String)> = transaction
            .query_row(
                "SELECT repository, standing_policy_version FROM jobs WHERE job_id = ?1",
                [brief.job_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((repository, policy_version)) = existing_scope {
            if repository != brief.repository || policy_version != brief.standing_policy_version {
                return Err(StoreError::JobScopeMismatch(brief.job_id.to_string()));
            }
        } else {
            transaction.execute(
                "INSERT INTO jobs (job_id, repository, standing_policy_version, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    brief.job_id.as_str(),
                    &brief.repository,
                    &brief.standing_policy_version,
                    now
                ],
            )?;
        }
        for dependency in &brief.dependencies {
            let exists: Option<i64> = transaction
                .query_row(
                    "SELECT 1 FROM units WHERE unit_id = ?1 AND job_id = ?2",
                    params![dependency.as_str(), brief.job_id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            if exists.is_none() {
                return Err(StoreError::UnknownDependency(dependency.to_string()));
            }
        }
        transaction.execute(
            "INSERT INTO units (unit_id, job_id, state, base_sha, updated_at) VALUES (?1, ?2, 'pending', ?3, ?4)",
            params![
                brief.unit_id.as_str(),
                brief.job_id.as_str(),
                brief.base_sha.as_str(),
                now
            ],
        )?;
        transaction.execute(
            "INSERT INTO unit_briefs (unit_id, brief_json) VALUES (?1, ?2)",
            params![brief.unit_id.as_str(), brief_json],
        )?;
        for dependency in &brief.dependencies {
            transaction.execute(
                "INSERT INTO dependencies (unit_id, depends_on_unit_id) VALUES (?1, ?2)",
                params![brief.unit_id.as_str(), dependency.as_str()],
            )?;
        }
        for grant in &brief.credential_grants {
            let methods_json = serde_json::to_string(&grant.methods)?;
            let existing_methods: Option<String> = transaction
                .query_row(
                    "SELECT methods_json FROM credential_grants WHERE job_id = ?1 AND credential_id = ?2",
                    params![brief.job_id.as_str(), &grant.credential_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing_methods) = existing_methods {
                if existing_methods != methods_json {
                    return Err(StoreError::CredentialGrantConflict(
                        grant.credential_id.clone(),
                    ));
                }
            } else {
                transaction.execute(
                    "INSERT INTO credential_grants (job_id, credential_id, methods_json, created_at) VALUES (?1, ?2, ?3, ?4)",
                    params![brief.job_id.as_str(), &grant.credential_id, methods_json, now],
                )?;
            }
        }
        append_event_tx(
            &transaction,
            &brief.job_id,
            &format!("unit-created:{}", brief.unit_id),
            "unit-created",
            &json!({"unit_id": brief.unit_id.as_str()}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns the current enforced state for a unit.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the unit does not exist or stored state is
    /// invalid.
    pub fn state(&self, unit_id: &UnitId) -> Result<JobState, StoreError> {
        let text: String = self
            .connection
            .query_row(
                "SELECT state FROM units WHERE unit_id = ?1",
                [unit_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::UnknownUnit(unit_id.to_string()))?;
        Ok(JobState::try_from(text.as_str())?)
    }

    /// Performs a non-evidence-bearing state edge atomically.
    ///
    /// Candidate creation, verification, integration completion, authorization,
    /// and merging use their dedicated methods and cannot be reached here.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for an illegal edge or missing unit.
    pub fn transition(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        next: JobState,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        if matches!(
            next,
            JobState::CandidateReady
                | JobState::Reviewing
                | JobState::Verified
                | JobState::Integrated
                | JobState::MergeAuthorized
                | JobState::Merged
        ) {
            return Err(StoreError::DedicatedGateRequired(next));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "actor": actor.as_str(),
            "unit_id": unit_id.as_str(),
            "next": next.as_str()
        });
        if command_replay_tx(
            &transaction,
            idempotency_key,
            "transition",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if matches!(current, JobState::Integrated | JobState::MergeAuthorized) {
            return Err(StoreError::DedicatedGateRequired(next));
        }
        if !current.can_transition_to(next) {
            return Err(StoreError::InvalidTransition { current, next });
        }
        let (capability, lease) = match next {
            JobState::Leased
            | JobState::FixRequired
            | JobState::Blocked
            | JobState::Abandoned
            | JobState::Quarantined
            | JobState::Superseded => (Capability::Coordinate, None),
            JobState::Grounding | JobState::Implementing | JobState::SelfVerifying => {
                (Capability::BranchPush, Some(LeaseKind::Profile))
            }
            JobState::IndependentlyVerifying => (Capability::Verify, Some(LeaseKind::Profile)),
            JobState::Integrating => (Capability::Integrate, Some(LeaseKind::Topology)),
            JobState::Pending
            | JobState::CandidateReady
            | JobState::Reviewing
            | JobState::Verified
            | JobState::Integrated
            | JobState::MergeAuthorized
            | JobState::Merged => return Err(StoreError::DedicatedGateRequired(next)),
        };
        require_unit_actor_tx(&transaction, actor, &job_id, unit_id, capability)?;
        if let Some(kind) = lease {
            require_actor_lease_tx(&transaction, actor, unit_id, kind, now)?;
        }
        update_state_tx(&transaction, unit_id, next, now)?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "state-transition",
            &json!({
                "unit_id": unit_id.as_str(),
                "actor": actor.as_str(),
                "from": current.as_str(),
                "to": next.as_str()
            }),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "transition",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Records the worker's committed candidate and enters `candidate-ready`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] unless the unit is self-verifying.
    pub fn record_candidate(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        head_sha: &Sha,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "actor": actor.as_str(),
            "unit_id": unit_id.as_str(),
            "head_sha": head_sha.as_str()
        });
        if command_replay_tx(
            &transaction,
            idempotency_key,
            "record-candidate",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::SelfVerifying {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::CandidateReady,
            });
        }
        require_unit_actor_tx(
            &transaction,
            actor,
            &job_id,
            unit_id,
            Capability::BranchPush,
        )?;
        require_actor_lease_tx(&transaction, actor, unit_id, LeaseKind::Profile, now)?;
        let tracked_head: Option<String> = transaction
            .query_row(
                "SELECT COALESCE(head_sha, base_sha) FROM branches WHERE unit_id = ?1",
                [unit_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(current_head) = tracked_head {
            let current_head = Sha::new(current_head)?;
            if current_head != *head_sha {
                return Err(StoreError::StaleBranchHead {
                    current: current_head,
                    expected: head_sha.clone(),
                });
            }
        }
        transaction.execute(
            "UPDATE units SET state = 'candidate-ready', candidate_sha = ?1, integration_sha = NULL, updated_at = ?2 WHERE unit_id = ?3",
            params![head_sha.as_str(), now, unit_id.as_str()],
        )?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "candidate-recorded",
            &json!({"unit_id": unit_id.as_str(), "actor": actor.as_str(), "head_sha": head_sha.as_str()}),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "record-candidate",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Publishes an exact-SHA verification verdict and enters review.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for stale SHA evidence or the wrong state.
    // coverage-critical
    pub fn record_verdict(
        &mut self,
        unit_id: &UnitId,
        verdict: &VerificationVerdict<'_>,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "unit_id": unit_id.as_str(),
            "verifier": verdict.verifier.as_str(),
            "head_sha": verdict.head_sha.as_str(),
            "passed": verdict.passed,
            "evidence": verdict.evidence
        });
        if command_replay_tx(
            &transaction,
            verdict.idempotency_key,
            "record-verdict",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::IndependentlyVerifying {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::Reviewing,
            });
        }
        let candidate = candidate_sha_tx(&transaction, unit_id)?;
        if candidate != *verdict.head_sha {
            return Err(StoreError::StaleVerification {
                candidate,
                verdict: verdict.head_sha.clone(),
            });
        }
        if !live_profile_has_unit_capability_tx(
            &transaction,
            verdict.verifier,
            &job_id,
            unit_id,
            Capability::Verify,
        )? {
            return Err(StoreError::UnauthorizedVerifier);
        }
        require_actor_lease_tx(
            &transaction,
            verdict.verifier,
            unit_id,
            LeaseKind::Profile,
            now,
        )?;
        transaction.execute(
            "INSERT INTO verification_verdicts (unit_id, verifier_profile, head_sha, passed, evidence_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                unit_id.as_str(),
                verdict.verifier.as_str(),
                verdict.head_sha.as_str(),
                verdict.passed,
                serde_json::to_string(&redact_evidence(verdict.evidence))?,
                now
            ],
        )?;
        update_state_tx(&transaction, unit_id, JobState::Reviewing, now)?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            verdict.idempotency_key,
            "verification-recorded",
            &json!({
                "unit_id": unit_id.as_str(),
                "verifier": verdict.verifier.as_str(),
                "head_sha": verdict.head_sha.as_str(),
                "passed": verdict.passed
            }),
            now,
        )?;
        record_command_tx(
            &transaction,
            verdict.idempotency_key,
            "record-verdict",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Marks a reviewed candidate verified only when its latest verdict passes
    /// at the exact current SHA. Reverified integration SHAs enter `integrated`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when prose is the only evidence or the verdict is
    /// stale or failing.
    // coverage-critical
    pub fn accept_verdict(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        idempotency_key: &str,
        now: i64,
    ) -> Result<JobState, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "actor": actor.as_str(),
            "unit_id": unit_id.as_str()
        });
        if let Some(result) = command_replay_tx(
            &transaction,
            idempotency_key,
            "accept-verdict",
            &command_request,
        )? {
            let state = result
                .as_str()
                .ok_or_else(|| StoreError::InvalidStoredCommand(idempotency_key.to_owned()))?;
            return Ok(JobState::try_from(state)?);
        }
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::Reviewing {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::Verified,
            });
        }
        require_unit_actor_tx(&transaction, actor, &job_id, unit_id, Capability::Integrate)?;
        require_actor_lease_tx(&transaction, actor, unit_id, LeaseKind::Profile, now)?;
        let candidate = candidate_sha_tx(&transaction, unit_id)?;
        require_passing_verdict_tx(&transaction, unit_id, &candidate)?;
        require_review_gate_tx(&transaction, unit_id, &candidate)?;
        let integration_sha: Option<String> = transaction.query_row(
            "SELECT integration_sha FROM units WHERE unit_id = ?1",
            [unit_id.as_str()],
            |row| row.get(0),
        )?;
        let next = if integration_sha.as_deref() == Some(candidate.as_str()) {
            JobState::Integrated
        } else {
            JobState::Verified
        };
        update_state_tx(&transaction, unit_id, next, now)?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "verdict-accepted",
            &json!({"unit_id": unit_id.as_str(), "actor": actor.as_str(), "head_sha": candidate.as_str(), "state": next.as_str()}),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "accept-verdict",
            &command_request,
            &json!(next.as_str()),
            event_id,
        )?;
        transaction.commit()?;
        Ok(next)
    }

    /// Completes integration. A content-changing SHA invalidates the old verdict
    /// and returns to `candidate-ready`; an unchanged SHA may become integrated.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] unless an integrator currently owns the state.
    pub fn complete_integration(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        integrated_sha: &Sha,
        idempotency_key: &str,
        now: i64,
    ) -> Result<JobState, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "actor": actor.as_str(),
            "unit_id": unit_id.as_str(),
            "integrated_sha": integrated_sha.as_str()
        });
        if let Some(result) = command_replay_tx(
            &transaction,
            idempotency_key,
            "complete-integration",
            &command_request,
        )? {
            let state = result
                .as_str()
                .ok_or_else(|| StoreError::InvalidStoredCommand(idempotency_key.to_owned()))?;
            return Ok(JobState::try_from(state)?);
        }
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::Integrating {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::Integrated,
            });
        }
        require_unit_actor_tx(&transaction, actor, &job_id, unit_id, Capability::Integrate)?;
        require_actor_lease_tx(&transaction, actor, unit_id, LeaseKind::Topology, now)?;
        let candidate = candidate_sha_tx(&transaction, unit_id)?;
        let next = if candidate == *integrated_sha {
            require_passing_verdict_tx(&transaction, unit_id, integrated_sha)?;
            JobState::Integrated
        } else {
            JobState::CandidateReady
        };
        transaction.execute(
            "UPDATE units SET state = ?1, candidate_sha = ?2, integration_sha = ?2, updated_at = ?3 WHERE unit_id = ?4",
            params![next.as_str(), integrated_sha.as_str(), now, unit_id.as_str()],
        )?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "integration-completed",
            &json!({
                "unit_id": unit_id.as_str(),
                "actor": actor.as_str(),
                "old_sha": candidate.as_str(),
                "integrated_sha": integrated_sha.as_str(),
                "requires_reverification": next == JobState::CandidateReady
            }),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "complete-integration",
            &command_request,
            &json!(next.as_str()),
            event_id,
        )?;
        transaction.commit()?;
        Ok(next)
    }

    /// Grants one exact-SHA merge authorization after integration verification.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for an unverified, stale, or unauthorized SHA.
    // coverage-critical
    pub fn authorize_merge(
        &mut self,
        unit_id: &UnitId,
        head_sha: &Sha,
        authorized_by: &ProfileId,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "unit_id": unit_id.as_str(),
            "head_sha": head_sha.as_str(),
            "authorized_by": authorized_by.as_str()
        });
        if command_replay_tx(
            &transaction,
            idempotency_key,
            "authorize-merge",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::Integrated {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::MergeAuthorized,
            });
        }
        if !live_profile_has_unit_capability_tx(
            &transaction,
            authorized_by,
            &job_id,
            unit_id,
            Capability::Merge,
        )? {
            return Err(StoreError::UnauthorizedShipper);
        }
        let candidate = candidate_sha_tx(&transaction, unit_id)?;
        if candidate != *head_sha {
            return Err(StoreError::UnauthorizedSha {
                expected: candidate,
                actual: head_sha.clone(),
            });
        }
        require_passing_verdict_tx(&transaction, unit_id, head_sha)?;
        transaction.execute(
            "INSERT INTO merge_authorizations (unit_id, head_sha, authorized_by, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![unit_id.as_str(), head_sha.as_str(), authorized_by.as_str(), now],
        )?;
        update_state_tx(&transaction, unit_id, JobState::MergeAuthorized, now)?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "merge-authorized",
            &json!({"unit_id": unit_id.as_str(), "head_sha": head_sha.as_str(), "authorized_by": authorized_by.as_str()}),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "authorize-merge",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Records protected-branch completion for exactly the authorized SHA.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for a stale SHA or missing authorization.
    // coverage-critical
    pub fn record_merged(
        &mut self,
        unit_id: &UnitId,
        head_sha: &Sha,
        merged_by: &ProfileId,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "unit_id": unit_id.as_str(),
            "head_sha": head_sha.as_str(),
            "merged_by": merged_by.as_str()
        });
        if command_replay_tx(
            &transaction,
            idempotency_key,
            "record-merged",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::MergeAuthorized {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::Merged,
            });
        }
        if !live_profile_has_unit_capability_tx(
            &transaction,
            merged_by,
            &job_id,
            unit_id,
            Capability::Merge,
        )? {
            return Err(StoreError::UnauthorizedShipper);
        }
        let authorized: Option<(String, String)> = transaction
            .query_row(
                "SELECT head_sha, authorized_by FROM merge_authorizations WHERE unit_id = ?1 AND invalidated_at IS NULL ORDER BY authorization_id DESC LIMIT 1",
                [unit_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (authorized_sha, authorized_actor) =
            authorized.ok_or(StoreError::MissingMergeAuthorization)?;
        let expected = Sha::new(authorized_sha)?;
        if expected != *head_sha {
            return Err(StoreError::UnauthorizedSha {
                expected,
                actual: head_sha.clone(),
            });
        }
        if authorized_actor != merged_by.as_str() {
            return Err(StoreError::UnauthorizedShipper);
        }
        update_state_tx(&transaction, unit_id, JobState::Merged, now)?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "merged",
            &json!({"unit_id": unit_id.as_str(), "head_sha": head_sha.as_str(), "merged_by": merged_by.as_str()}),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "record-merged",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Recovers an integrated or merge-authorized unit after an external merge
    /// failure while preserving and invalidating its exact-SHA authorization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] unless the actor has the capability belonging to
    /// the protected state and the requested recovery state is explicit.
    // coverage-critical
    pub fn recover_integration(
        &mut self,
        unit_id: &UnitId,
        actor: &ProfileId,
        next: JobState,
        reason: &Value,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "unit_id": unit_id.as_str(),
            "actor": actor.as_str(),
            "next": next.as_str(),
            "reason": reason
        });
        if command_replay_tx(
            &transaction,
            idempotency_key,
            "recover-integration",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if !matches!(
            next,
            JobState::FixRequired
                | JobState::Blocked
                | JobState::Abandoned
                | JobState::Quarantined
                | JobState::Superseded
        ) || !current.can_transition_to(next)
        {
            return Err(StoreError::InvalidRecovery { current, next });
        }
        let capability = match current {
            JobState::Integrated => Capability::Integrate,
            JobState::MergeAuthorized => Capability::Merge,
            _ => return Err(StoreError::InvalidRecovery { current, next }),
        };
        if !live_profile_has_unit_capability_tx(&transaction, actor, &job_id, unit_id, capability)?
        {
            return Err(StoreError::UnauthorizedRecovery);
        }
        if current == JobState::MergeAuthorized {
            transaction.execute(
                "UPDATE merge_authorizations SET invalidated_at = ?1 WHERE unit_id = ?2 AND invalidated_at IS NULL",
                params![now, unit_id.as_str()],
            )?;
        }
        transaction.execute(
            "UPDATE leases SET released_at = ?1 WHERE unit_id = ?2 AND kind = 'topology' AND released_at IS NULL",
            params![now, unit_id.as_str()],
        )?;
        let candidate = candidate_sha_tx(&transaction, unit_id)?;
        update_state_tx(&transaction, unit_id, next, now)?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "integration-recovered",
            &json!({
                "unit_id": unit_id.as_str(),
                "actor": actor.as_str(),
                "from": current.as_str(),
                "to": next.as_str(),
                "head_sha": candidate.as_str(),
                "reason": reason
            }),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "recover-integration",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Acquires one expiring path, profile, or topology lease.
    ///
    /// Expired leases are closed in the same transaction. Path-prefix overlap,
    /// duplicate profiles, and a second topology owner are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when another live lease overlaps or expiry is not
    /// in the future.
    #[expect(
        clippy::too_many_arguments,
        reason = "the lease transaction keeps both actors and every immutable lease operand explicit"
    )]
    // coverage-critical
    pub fn acquire_lease(
        &mut self,
        coordinator: &ProfileId,
        holder: &ProfileId,
        job_id: &JobId,
        unit_id: &UnitId,
        kind: LeaseKind,
        resource: &str,
        expires_at: i64,
        now: i64,
    ) -> Result<i64, StoreError> {
        if resource.trim().is_empty()
            || expires_at <= now
            || (kind == LeaseKind::Path && !is_safe_relative_path(Path::new(resource)))
        {
            return Err(StoreError::InvalidLease);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (actual_job, _) = unit_identity_tx(&transaction, unit_id)?;
        if actual_job != *job_id {
            return Err(StoreError::JobUnitMismatch);
        }
        if !live_profile_has_capability_tx(
            &transaction,
            coordinator,
            job_id,
            Capability::Coordinate,
        )? {
            return Err(StoreError::UnauthorizedCoordinator);
        }
        let holder_capability = match kind {
            LeaseKind::Path => Capability::RepositoryWrite,
            LeaseKind::Topology => Capability::Integrate,
            LeaseKind::Profile => Capability::EvidenceWrite,
        };
        require_unit_actor_tx(&transaction, holder, job_id, unit_id, holder_capability)?;
        if kind == LeaseKind::Profile && resource != holder.as_str() {
            return Err(StoreError::InvalidLease);
        }
        let unsatisfied: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM dependencies AS dependency JOIN units AS prerequisite ON prerequisite.unit_id = dependency.depends_on_unit_id WHERE dependency.unit_id = ?1 AND prerequisite.state != 'merged'",
            [unit_id.as_str()],
            |row| row.get(0),
        )?;
        if unsatisfied != 0 {
            return Err(StoreError::DependenciesUnsatisfied(unsatisfied));
        }
        transaction.execute(
            "UPDATE leases SET released_at = ?1 WHERE released_at IS NULL AND expires_at <= ?1",
            [now],
        )?;
        let mut statement = transaction.prepare(
            "SELECT job_id, resource FROM leases WHERE kind = ?1 AND released_at IS NULL AND expires_at > ?2",
        )?;
        let resources = statement
            .query_map(params![kind.as_str(), now], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let overlaps = match kind {
            LeaseKind::Path => resources
                .iter()
                .any(|(_, active)| path_resources_overlap(active, resource)),
            LeaseKind::Topology => resources
                .iter()
                .any(|(active_job, _)| active_job == job_id.as_str()),
            LeaseKind::Profile => resources.iter().any(|(_, active)| active == resource),
        };
        if overlaps {
            return Err(StoreError::LeaseConflict(resource.to_owned()));
        }
        transaction.execute(
            "INSERT INTO leases (job_id, unit_id, kind, resource, expires_at, holder_profile) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![job_id.as_str(), unit_id.as_str(), kind.as_str(), resource, expires_at, holder.as_str()],
        )?;
        let lease_id = transaction.last_insert_rowid();
        append_event_tx(
            &transaction,
            job_id,
            &format!("lease-acquired:{lease_id}"),
            "lease-acquired",
            &json!({"unit_id": unit_id.as_str(), "lease_id": lease_id, "kind": kind.as_str(), "resource": resource, "coordinator": coordinator.as_str(), "holder": holder.as_str()}),
            now,
        )?;
        transaction.commit()?;
        Ok(lease_id)
    }

    /// Accepts a worker result only under its current live lease.
    ///
    /// Late results are durably quarantined before the stale-result error is
    /// returned.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::StaleLease`] for missing, released, or expired
    /// leases.
    // coverage-critical
    pub fn accept_worker_result(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        lease_id: i64,
        head_sha: &Sha,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        require_unit_actor_tx(
            &transaction,
            actor,
            &job_id,
            unit_id,
            Capability::EvidenceWrite,
        )?;
        let live: bool = transaction
            .query_row(
                "SELECT expires_at > ?1 AND released_at IS NULL FROM leases WHERE lease_id = ?2 AND unit_id = ?3 AND holder_profile = ?4 AND kind = 'profile'",
                params![now, lease_id, unit_id.as_str(), actor.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false);
        if !live {
            let replacement_live: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM leases WHERE unit_id = ?1 AND kind = 'profile' AND released_at IS NULL AND expires_at > ?2)",
                params![unit_id.as_str(), now],
                |row| row.get(0),
            )?;
            if !replacement_live && current.can_transition_to(JobState::Quarantined) {
                update_state_tx(&transaction, unit_id, JobState::Quarantined, now)?;
            }
            append_event_tx(
                &transaction,
                &job_id,
                &format!("stale-result:{lease_id}:{}", head_sha.as_str()),
                "result-quarantined",
                &json!({"unit_id": unit_id.as_str(), "actor": actor.as_str(), "lease_id": lease_id, "head_sha": head_sha.as_str()}),
                now,
            )?;
            transaction.commit()?;
            return Err(StoreError::StaleLease(lease_id));
        }
        append_event_tx(
            &transaction,
            &job_id,
            &format!("worker-result:{lease_id}:{}", head_sha.as_str()),
            "worker-result",
            &json!({"unit_id": unit_id.as_str(), "actor": actor.as_str(), "lease_id": lease_id, "head_sha": head_sha.as_str()}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Appends an idempotent evidence event.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for an empty key or database failure.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "tests exercise the private event primitive; production mutations use authorized transactional helpers"
        )
    )]
    pub(crate) fn append_event(
        &mut self,
        job_id: &JobId,
        idempotency_key: &str,
        event_type: &str,
        payload: &Value,
        now: i64,
    ) -> Result<i64, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = append_event_tx(
            &transaction,
            job_id,
            idempotency_key,
            event_type,
            payload,
            now,
        )?;
        transaction.commit()?;
        Ok(id)
    }

    /// Validates a worker report against the immutable brief's object schema and
    /// appends its redacted evidence form.
    ///
    /// The step-1 validator deliberately supports the contract's required
    /// object fields. Unsupported or missing schema shapes fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the report is not an object, a required field
    /// is missing, or persistence fails.
    pub fn record_report(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        report: &Value,
        idempotency_key: &str,
        now: i64,
    ) -> Result<i64, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "actor": actor.as_str(),
            "unit_id": unit_id.as_str(),
            "report": report
        });
        if let Some(result) = command_replay_tx(
            &transaction,
            idempotency_key,
            "record-report",
            &command_request,
        )? {
            return result
                .as_i64()
                .ok_or_else(|| StoreError::InvalidStoredCommand(idempotency_key.to_owned()));
        }
        let (job_id, _) = unit_identity_tx(&transaction, unit_id)?;
        require_unit_actor_tx(
            &transaction,
            actor,
            &job_id,
            unit_id,
            Capability::EvidenceWrite,
        )?;
        require_actor_lease_tx(&transaction, actor, unit_id, LeaseKind::Profile, now)?;
        let brief_json: String = transaction.query_row(
            "SELECT brief_json FROM unit_briefs WHERE unit_id = ?1",
            [unit_id.as_str()],
            |row| row.get(0),
        )?;
        let brief: JobBrief = serde_json::from_str(&brief_json)?;
        if !report_matches_schema(&brief.report_schema, report) {
            return Err(StoreError::ReportSchemaViolation);
        }
        let id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "worker-report",
            &json!({"actor": actor.as_str(), "report": report}),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "record-report",
            &command_request,
            &json!(id),
            id,
        )?;
        transaction.commit()?;
        Ok(id)
    }

    /// Registers one role-bound isolated profile home.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for a job/unit mismatch, relative home, duplicate
    /// profile, or database failure.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "trusted scheduler provisioning remains private until its command adapter is implemented"
        )
    )]
    pub(crate) fn register_profile(
        &mut self,
        profile_id: &ProfileId,
        job_id: &JobId,
        unit_id: &UnitId,
        role: Role,
        home: &Path,
        now: i64,
    ) -> Result<(), StoreError> {
        if !is_normalized_absolute_path(home) {
            return Err(StoreError::InvalidProfileHome(home.to_path_buf()));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (actual_job, _) = unit_identity_tx(&transaction, unit_id)?;
        if actual_job != *job_id {
            return Err(StoreError::JobUnitMismatch);
        }
        transaction.execute(
            "INSERT INTO profiles (profile_id, job_id, unit_id, role, home) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                profile_id.as_str(),
                job_id.as_str(),
                unit_id.as_str(),
                role.as_str(),
                home.to_string_lossy()
            ],
        )?;
        append_event_tx(
            &transaction,
            job_id,
            &format!("profile-registered:{profile_id}"),
            "profile-registered",
            &json!({"profile_id": profile_id.as_str(), "unit_id": unit_id.as_str(), "role": role.as_str()}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Registers a deterministic session key under exactly one profile.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for a blank key, unknown profile, duplicate, or
    /// database failure.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "trusted scheduler provisioning remains private until its command adapter is implemented"
        )
    )]
    pub(crate) fn register_session(
        &mut self,
        session_id: &SessionId,
        profile_id: &ProfileId,
        external_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        if external_key.trim().is_empty() {
            return Err(StoreError::InvalidSessionKey);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job_id: Option<String> = transaction
            .query_row(
                "SELECT job_id FROM profiles WHERE profile_id = ?1 AND destroyed_at IS NULL",
                [profile_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let job_id =
            job_id.ok_or_else(|| StoreError::UnknownLiveProfile(profile_id.to_string()))?;
        transaction.execute(
            "INSERT INTO sessions (session_id, profile_id, external_key, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_id.as_str(), profile_id.as_str(), external_key, now],
        )?;
        append_event_tx(
            &transaction,
            &JobId::new(job_id)?,
            &format!("session-registered:{session_id}"),
            "session-registered",
            &json!({"session_id": session_id.as_str(), "profile_id": profile_id.as_str()}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Marks an isolated profile and all of its sessions destroyed.
    ///
    /// Filesystem removal remains the scheduler's responsibility; this method
    /// first durably removes the profile's control-plane authority and releases
    /// any matching profile lease.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] unless a live same-job coordinator owns the
    /// teardown or the target profile is already absent/destroyed.
    pub fn destroy_profile(
        &mut self,
        coordinator: &ProfileId,
        target: &ProfileId,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "coordinator": coordinator.as_str(),
            "target": target.as_str()
        });
        if command_replay_tx(
            &transaction,
            idempotency_key,
            "destroy-profile",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let target_record: Option<(String, String)> = transaction
            .query_row(
                "SELECT job_id, unit_id FROM profiles WHERE profile_id = ?1 AND destroyed_at IS NULL",
                [target.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (job_id, unit_id) =
            target_record.ok_or_else(|| StoreError::UnknownLiveProfile(target.to_string()))?;
        if !live_profile_has_capability_tx(
            &transaction,
            coordinator,
            &JobId::new(job_id.clone())?,
            Capability::Coordinate,
        )? {
            return Err(StoreError::UnauthorizedCoordinator);
        }
        transaction.execute(
            "UPDATE profiles SET destroyed_at = ?1 WHERE profile_id = ?2",
            params![now, target.as_str()],
        )?;
        transaction.execute(
            "UPDATE sessions SET destroyed_at = ?1 WHERE profile_id = ?2 AND destroyed_at IS NULL",
            params![now, target.as_str()],
        )?;
        transaction.execute(
            "UPDATE leases SET released_at = ?1 WHERE job_id = ?2 AND unit_id = ?3 AND holder_profile = ?4 AND released_at IS NULL",
            params![now, &job_id, &unit_id, target.as_str()],
        )?;
        let job_id = JobId::new(job_id)?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "profile-destroyed",
            &json!({
                "profile_id": target.as_str(),
                "coordinator": coordinator.as_str(),
                "unit_id": unit_id
            }),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "destroy-profile",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Reports whether one exact method remains granted to a job.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] if persisted grant methods are invalid JSON or
    /// the database query fails.
    pub fn credential_method_is_active(
        &self,
        job_id: &JobId,
        credential_id: &str,
        method: &str,
    ) -> Result<bool, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT methods_json FROM credential_grants WHERE job_id = ?1 AND credential_id = ?2 AND revoked_at IS NULL",
        )?;
        let rows = statement.query_map(params![job_id.as_str(), credential_id], |row| {
            row.get::<_, String>(0)
        })?;
        for row in rows {
            let methods: Vec<String> = serde_json::from_str(&row?)?;
            if methods.iter().any(|granted| granted == method) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Revokes every active instance of an opaque credential grant.
    ///
    /// Only a live coordinator profile belonging to the same job may perform
    /// the revocation. Secret material never enters this repository.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for an unauthorized coordinator, unknown active
    /// grant, conflicting idempotency key, or database failure.
    // coverage-critical
    pub fn revoke_credential_grant(
        &mut self,
        job_id: &JobId,
        coordinator: &ProfileId,
        credential_id: &str,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "job_id": job_id.as_str(),
            "coordinator": coordinator.as_str(),
            "credential_id": credential_id
        });
        if command_replay_tx(
            &transaction,
            idempotency_key,
            "revoke-credential-grant",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        if !live_profile_has_capability_tx(
            &transaction,
            coordinator,
            job_id,
            Capability::Coordinate,
        )? {
            return Err(StoreError::UnauthorizedCoordinator);
        }
        let changed = transaction.execute(
            "UPDATE credential_grants SET revoked_at = ?1 WHERE job_id = ?2 AND credential_id = ?3 AND revoked_at IS NULL",
            params![now, job_id.as_str(), credential_id],
        )?;
        if changed == 0 {
            return Err(StoreError::UnknownActiveCredentialGrant(
                credential_id.to_owned(),
            ));
        }
        let event_id = append_event_tx(
            &transaction,
            job_id,
            idempotency_key,
            "credential-revoked",
            &json!({"credential_id": credential_id, "coordinator": coordinator.as_str()}),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "revoke-credential-grant",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Registers the one branch/worktree assigned to a coding unit.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] unless branch namespace and base SHA exactly match
    /// the immutable job record and the worktree path is absolute.
    // coverage-critical
    pub fn register_branch(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        name: &str,
        worktree: &Path,
        base_sha: &Sha,
        now: i64,
    ) -> Result<(), StoreError> {
        if !is_normalized_absolute_path(worktree) {
            return Err(StoreError::InvalidWorktree(worktree.to_path_buf()));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, _) = unit_identity_tx(&transaction, unit_id)?;
        require_unit_actor_tx(
            &transaction,
            actor,
            &job_id,
            unit_id,
            Capability::BranchPush,
        )?;
        require_actor_lease_tx(&transaction, actor, unit_id, LeaseKind::Profile, now)?;
        let expected_name = format!("nswarm/{job_id}/{unit_id}");
        let expected_base: String = transaction.query_row(
            "SELECT base_sha FROM units WHERE unit_id = ?1",
            [unit_id.as_str()],
            |row| row.get(0),
        )?;
        if name != expected_name || expected_base != base_sha.as_str() {
            return Err(StoreError::InvalidBranchAssignment);
        }
        transaction.execute(
            "INSERT INTO branches (unit_id, name, worktree, base_sha, head_sha) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![
                unit_id.as_str(),
                name,
                worktree.to_string_lossy(),
                base_sha.as_str()
            ],
        )?;
        append_event_tx(
            &transaction,
            &job_id,
            &format!("branch-registered:{unit_id}"),
            "branch-registered",
            &json!({"unit_id": unit_id.as_str(), "actor": actor.as_str(), "name": name, "base_sha": base_sha.as_str()}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Advances a registered coding branch using compare-and-swap semantics.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] outside coding states, for an unknown branch, or
    /// when the caller's expected head is stale.
    // coverage-critical
    pub fn update_branch_head(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        expected_head: &Sha,
        new_head: &Sha,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "actor": actor.as_str(),
            "unit_id": unit_id.as_str(),
            "expected_head": expected_head.as_str(),
            "new_head": new_head.as_str()
        });
        if command_replay_tx(
            &transaction,
            idempotency_key,
            "update-branch-head",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let (job_id, state) = unit_identity_tx(&transaction, unit_id)?;
        require_unit_actor_tx(
            &transaction,
            actor,
            &job_id,
            unit_id,
            Capability::BranchPush,
        )?;
        require_actor_lease_tx(&transaction, actor, unit_id, LeaseKind::Profile, now)?;
        if !matches!(state, JobState::Implementing | JobState::SelfVerifying) {
            return Err(StoreError::BranchUpdateOutsideCodingState(state));
        }
        let current: Option<String> = transaction
            .query_row(
                "SELECT COALESCE(head_sha, base_sha) FROM branches WHERE unit_id = ?1",
                [unit_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let current = current.ok_or_else(|| StoreError::UnknownBranch(unit_id.to_string()))?;
        let current = Sha::new(current)?;
        if current != *expected_head {
            return Err(StoreError::StaleBranchHead {
                current,
                expected: expected_head.clone(),
            });
        }
        transaction.execute(
            "UPDATE branches SET head_sha = ?1 WHERE unit_id = ?2",
            params![new_head.as_str(), unit_id.as_str()],
        )?;
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "branch-head-updated",
            &json!({
                "unit_id": unit_id.as_str(),
                "actor": actor.as_str(),
                "previous_head": expected_head.as_str(),
                "head_sha": new_head.as_str()
            }),
            now,
        )?;
        record_command_tx(
            &transaction,
            idempotency_key,
            "update-branch-head",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Stores an immutable artifact digest for later verification.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for an unsafe path, stale source SHA, duplicate,
    /// or database failure.
    #[expect(
        clippy::too_many_arguments,
        reason = "artifact attribution keeps the actor and exact evidence identity explicit"
    )]
    // coverage-critical
    pub fn record_artifact(
        &mut self,
        actor: &ProfileId,
        unit_id: &UnitId,
        kind: ArtifactKind,
        path: &Path,
        head_sha: &Sha,
        digest: &Sha,
        now: i64,
    ) -> Result<i64, StoreError> {
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(StoreError::InvalidArtifact);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, _) = unit_identity_tx(&transaction, unit_id)?;
        require_unit_actor_tx(
            &transaction,
            actor,
            &job_id,
            unit_id,
            Capability::EvidenceWrite,
        )?;
        require_actor_lease_tx(&transaction, actor, unit_id, LeaseKind::Profile, now)?;
        let current: String = transaction.query_row(
            "SELECT COALESCE(candidate_sha, base_sha) FROM units WHERE unit_id = ?1",
            [unit_id.as_str()],
            |row| row.get(0),
        )?;
        let current = Sha::new(current)?;
        if current != *head_sha {
            return Err(StoreError::StaleArtifact {
                current,
                artifact: head_sha.clone(),
            });
        }
        transaction.execute(
            "INSERT INTO artifacts (unit_id, kind, path, digest, created_at, head_sha) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![unit_id.as_str(), kind.as_str(), path.to_string_lossy(), digest.as_str(), now, head_sha.as_str()],
        )?;
        let artifact_id = transaction.last_insert_rowid();
        append_event_tx(
            &transaction,
            &job_id,
            &format!("artifact-recorded:{artifact_id}"),
            "artifact-recorded",
            &json!({
                "unit_id": unit_id.as_str(),
                "actor": actor.as_str(),
                "artifact_id": artifact_id,
                "kind": kind.as_str(),
                "head_sha": head_sha.as_str(),
                "digest": digest.as_str()
            }),
            now,
        )?;
        transaction.commit()?;
        Ok(artifact_id)
    }

    /// Records an independent reviewer assessment against the exact candidate.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] unless the unit is in review, the SHA is current,
    /// and the profile has the verifier/reviewer role for the same job.
    // coverage-critical
    pub fn record_review(
        &mut self,
        unit_id: &UnitId,
        reviewer: &ProfileId,
        head_sha: &Sha,
        assessment: ReviewAssessment,
        finding: &Value,
        now: i64,
    ) -> Result<i64, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, state) = unit_identity_tx(&transaction, unit_id)?;
        if state != JobState::Reviewing {
            return Err(StoreError::ReviewOutsideReviewState);
        }
        let candidate = candidate_sha_tx(&transaction, unit_id)?;
        if candidate != *head_sha {
            return Err(StoreError::StaleVerification {
                candidate,
                verdict: head_sha.clone(),
            });
        }
        if !live_profile_has_unit_capability_tx(
            &transaction,
            reviewer,
            &job_id,
            unit_id,
            Capability::Verify,
        )? {
            return Err(StoreError::UnauthorizedReviewer);
        }
        require_actor_lease_tx(&transaction, reviewer, unit_id, LeaseKind::Profile, now)?;
        transaction.execute(
            "INSERT INTO review_findings (unit_id, head_sha, reviewer_profile, severity, finding_json, disposition, created_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
            params![unit_id.as_str(), head_sha.as_str(), reviewer.as_str(), assessment.as_str(), serde_json::to_string(&redact_evidence(finding))?, now],
        )?;
        let finding_id = transaction.last_insert_rowid();
        append_event_tx(
            &transaction,
            &job_id,
            &format!("review-recorded:{finding_id}"),
            "review-recorded",
            &json!({"unit_id": unit_id.as_str(), "reviewer": reviewer.as_str(), "head_sha": head_sha.as_str(), "assessment": assessment.as_str()}),
            now,
        )?;
        transaction.commit()?;
        Ok(finding_id)
    }

    /// Records the integrator-owned final disposition for one review finding.
    ///
    /// Disposition is one-shot and exact-candidate scoped. The rationale is
    /// retained only in the redacted append-only event ledger.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for the wrong state, unauthorized profile, stale
    /// or already-disposed finding, or persistence failure.
    // coverage-critical
    pub fn dispose_review_finding(
        &mut self,
        unit_id: &UnitId,
        integrator: &ProfileId,
        request: &FindingDisposition<'_>,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command_request = json!({
            "unit_id": unit_id.as_str(),
            "integrator": integrator.as_str(),
            "finding_id": request.finding_id,
            "disposition": request.disposition.as_str(),
            "rationale": request.rationale
        });
        if command_replay_tx(
            &transaction,
            request.idempotency_key,
            "dispose-review-finding",
            &command_request,
        )?
        .is_some()
        {
            return Ok(());
        }
        let (job_id, state) = unit_identity_tx(&transaction, unit_id)?;
        if state != JobState::Reviewing {
            return Err(StoreError::ReviewOutsideReviewState);
        }
        if !live_profile_has_unit_capability_tx(
            &transaction,
            integrator,
            &job_id,
            unit_id,
            Capability::Integrate,
        )? {
            return Err(StoreError::UnauthorizedIntegrator);
        }
        require_actor_lease_tx(&transaction, integrator, unit_id, LeaseKind::Profile, now)?;
        let candidate = candidate_sha_tx(&transaction, unit_id)?;
        let changed = transaction.execute(
            "UPDATE review_findings SET disposition = ?1 WHERE finding_id = ?2 AND unit_id = ?3 AND head_sha = ?4 AND disposition IS NULL",
            params![
                request.disposition.as_str(),
                request.finding_id,
                unit_id.as_str(),
                candidate.as_str()
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::UnknownOpenFinding(request.finding_id));
        }
        let event_id = append_event_tx(
            &transaction,
            &job_id,
            request.idempotency_key,
            "review-disposed",
            &json!({
                "unit_id": unit_id.as_str(),
                "finding_id": request.finding_id,
                "integrator": integrator.as_str(),
                "disposition": request.disposition.as_str(),
                "rationale": request.rationale
            }),
            now,
        )?;
        record_command_tx(
            &transaction,
            request.idempotency_key,
            "dispose-review-finding",
            &command_request,
            &Value::Null,
            event_id,
        )?;
        transaction.commit()?;
        Ok(())
    }
}

// coverage-critical
fn live_profile_has_capability_tx(
    transaction: &rusqlite::Transaction<'_>,
    profile_id: &ProfileId,
    job_id: &JobId,
    capability: Capability,
) -> Result<bool, StoreError> {
    let role: Option<String> = transaction
        .query_row(
            "SELECT role FROM profiles WHERE profile_id = ?1 AND job_id = ?2 AND destroyed_at IS NULL",
            params![profile_id.as_str(), job_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(role
        .as_deref()
        .and_then(Role::from_name)
        .is_some_and(|role| role.can(capability)))
}

fn live_profile_has_unit_capability_tx(
    transaction: &rusqlite::Transaction<'_>,
    profile_id: &ProfileId,
    job_id: &JobId,
    unit_id: &UnitId,
    capability: Capability,
) -> Result<bool, StoreError> {
    let role: Option<String> = transaction
        .query_row(
            "SELECT role FROM profiles WHERE profile_id = ?1 AND job_id = ?2 AND unit_id = ?3 AND destroyed_at IS NULL",
            params![profile_id.as_str(), job_id.as_str(), unit_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(role
        .as_deref()
        .and_then(Role::from_name)
        .is_some_and(|role| role.can(capability)))
}

fn require_unit_actor_tx(
    transaction: &rusqlite::Transaction<'_>,
    actor: &ProfileId,
    job_id: &JobId,
    unit_id: &UnitId,
    capability: Capability,
) -> Result<(), StoreError> {
    if live_profile_has_unit_capability_tx(transaction, actor, job_id, unit_id, capability)? {
        Ok(())
    } else {
        Err(StoreError::UnauthorizedActor(actor.to_string()))
    }
}

fn require_actor_lease_tx(
    transaction: &rusqlite::Transaction<'_>,
    actor: &ProfileId,
    unit_id: &UnitId,
    kind: LeaseKind,
    now: i64,
) -> Result<(), StoreError> {
    let live: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM leases WHERE holder_profile = ?1 AND unit_id = ?2 AND kind = ?3 AND released_at IS NULL AND expires_at > ?4)",
        params![actor.as_str(), unit_id.as_str(), kind.as_str(), now],
        |row| row.get(0),
    )?;
    if live {
        Ok(())
    } else {
        Err(StoreError::MissingActorLease {
            actor: actor.to_string(),
            kind,
        })
    }
}

fn unit_identity_tx(
    transaction: &rusqlite::Transaction<'_>,
    unit_id: &UnitId,
) -> Result<(JobId, JobState), StoreError> {
    let row: Option<(String, String)> = transaction
        .query_row(
            "SELECT job_id, state FROM units WHERE unit_id = ?1",
            [unit_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (job_id, state) = row.ok_or_else(|| StoreError::UnknownUnit(unit_id.to_string()))?;
    Ok((JobId::new(job_id)?, JobState::try_from(state.as_str())?))
}

fn candidate_sha_tx(
    transaction: &rusqlite::Transaction<'_>,
    unit_id: &UnitId,
) -> Result<Sha, StoreError> {
    let value: Option<String> = transaction.query_row(
        "SELECT candidate_sha FROM units WHERE unit_id = ?1",
        [unit_id.as_str()],
        |row| row.get(0),
    )?;
    value
        .map(Sha::new)
        .transpose()?
        .ok_or(StoreError::MissingCandidate)
}

// coverage-critical
fn require_passing_verdict_tx(
    transaction: &rusqlite::Transaction<'_>,
    unit_id: &UnitId,
    head_sha: &Sha,
) -> Result<(), StoreError> {
    let (attributed, failed): (i64, i64) = transaction.query_row(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN passed = 0 THEN 1 ELSE 0 END), 0) FROM verification_verdicts WHERE unit_id = ?1 AND head_sha = ?2 AND verifier_profile IS NOT NULL",
        params![unit_id.as_str(), head_sha.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if attributed > 0 && failed == 0 {
        Ok(())
    } else {
        Err(StoreError::MissingPassingVerdict(head_sha.clone()))
    }
}

// coverage-critical
fn require_review_gate_tx(
    transaction: &rusqlite::Transaction<'_>,
    unit_id: &UnitId,
    head_sha: &Sha,
) -> Result<(), StoreError> {
    let brief_json: String = transaction.query_row(
        "SELECT brief_json FROM unit_briefs WHERE unit_id = ?1",
        [unit_id.as_str()],
        |row| row.get(0),
    )?;
    let brief: JobBrief = serde_json::from_str(&brief_json)?;
    if brief.risk_class == RiskClass::Low {
        return Ok(());
    }
    let (reviewers, unresolved, blocking): (i64, i64, i64) = transaction.query_row(
        "SELECT COUNT(DISTINCT reviewer_profile), COALESCE(SUM(CASE WHEN disposition IS NULL THEN 1 ELSE 0 END), 0), COALESCE(SUM(CASE WHEN disposition = 'blocking' THEN 1 ELSE 0 END), 0) FROM review_findings WHERE unit_id = ?1 AND head_sha = ?2",
        params![unit_id.as_str(), head_sha.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if reviewers < 2 || unresolved != 0 || blocking != 0 {
        return Err(StoreError::ReviewGateUnsatisfied {
            reviewers,
            unresolved,
            blocking,
        });
    }
    Ok(())
}

fn update_state_tx(
    transaction: &rusqlite::Transaction<'_>,
    unit_id: &UnitId,
    state: JobState,
    now: i64,
) -> Result<(), StoreError> {
    transaction.execute(
        "UPDATE units SET state = ?1, updated_at = ?2 WHERE unit_id = ?3",
        params![state.as_str(), now, unit_id.as_str()],
    )?;
    Ok(())
}

fn command_replay_tx(
    transaction: &rusqlite::Transaction<'_>,
    idempotency_key: &str,
    command_type: &str,
    request: &Value,
) -> Result<Option<Value>, StoreError> {
    if idempotency_key.trim().is_empty() || command_type.trim().is_empty() {
        return Err(StoreError::InvalidEvent);
    }
    let request_json = serde_json::to_string(&redact_evidence(request))?;
    let existing: Option<(String, String, String)> = transaction
        .query_row(
            "SELECT command_type, request_json, result_json FROM command_results WHERE idempotency_key = ?1",
            [idempotency_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((existing_type, existing_request, result_json)) = existing else {
        return Ok(None);
    };
    if existing_type != command_type || existing_request != request_json {
        return Err(StoreError::IdempotencyConflict(idempotency_key.to_owned()));
    }
    Ok(Some(serde_json::from_str(&result_json)?))
}

fn record_command_tx(
    transaction: &rusqlite::Transaction<'_>,
    idempotency_key: &str,
    command_type: &str,
    request: &Value,
    result: &Value,
    event_id: i64,
) -> Result<(), StoreError> {
    let inserted = transaction.execute(
        "INSERT INTO command_results (idempotency_key, job_id, command_type, request_json, result_json, event_id, created_at) SELECT ?1, job_id, ?2, ?3, ?4, event_id, created_at FROM events WHERE event_id = ?5",
        params![
            idempotency_key,
            command_type,
            serde_json::to_string(&redact_evidence(request))?,
            serde_json::to_string(&redact_evidence(result))?,
            event_id
        ],
    )?;
    if inserted != 1 {
        return Err(StoreError::InvalidStoredCommand(idempotency_key.to_owned()));
    }
    Ok(())
}

// coverage-critical
fn append_event_tx(
    transaction: &rusqlite::Transaction<'_>,
    job_id: &JobId,
    idempotency_key: &str,
    event_type: &str,
    payload: &Value,
    now: i64,
) -> Result<i64, StoreError> {
    if idempotency_key.trim().is_empty() || event_type.trim().is_empty() {
        return Err(StoreError::InvalidEvent);
    }
    let payload_json = serde_json::to_string(&redact_evidence(payload))?;
    let existing: Option<(i64, String, String, String)> = transaction
        .query_row(
            "SELECT event_id, job_id, event_type, payload_json FROM events WHERE idempotency_key = ?1",
            [idempotency_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    if let Some((event_id, existing_job, existing_type, existing_payload)) = existing {
        if existing_job == job_id.as_str()
            && existing_type == event_type
            && existing_payload == payload_json
        {
            return Ok(event_id);
        }
        return Err(StoreError::IdempotencyConflict(idempotency_key.to_owned()));
    }
    transaction.execute(
        "INSERT INTO events (job_id, idempotency_key, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![job_id.as_str(), idempotency_key, event_type, payload_json, now],
    )?;
    let id = transaction.query_row(
        "SELECT event_id FROM events WHERE idempotency_key = ?1",
        [idempotency_key],
        |row| row.get(0),
    )?;
    Ok(id)
}

fn path_resources_overlap(left: &str, right: &str) -> bool {
    let left = Path::new(left);
    let right = Path::new(right);
    left.starts_with(right) || right.starts_with(left)
}

// coverage-critical
fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

// coverage-critical
fn is_normalized_absolute_path(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
}

// coverage-critical
fn redact_evidence(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, item)| {
                    let normalized = key
                        .chars()
                        .filter(char::is_ascii_alphanumeric)
                        .flat_map(char::to_lowercase)
                        .collect::<String>();
                    let secret_field = [
                        "key",
                        "token",
                        "password",
                        "passphrase",
                        "secret",
                        "credential",
                        "authorization",
                        "cookie",
                    ]
                    .iter()
                    .any(|name| normalized == *name || normalized.ends_with(name));
                    (
                        key.clone(),
                        if secret_field {
                            Value::String("[REDACTED]".to_owned())
                        } else {
                            redact_evidence(item)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_evidence).collect()),
        Value::String(text) if contains_secret_shape(text) => {
            Value::String("[REDACTED]".to_owned())
        }
        _ => value.clone(),
    }
}

// coverage-critical
fn contains_secret_shape(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.contains("PRIVATE KEY")
        || upper.split("BEARER ").skip(1).any(|remainder| {
            remainder
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .count()
                >= 20
        })
        || text
            .split(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
            })
            .any(|word| {
                ((word.starts_with("ghp_")
                    || word.starts_with("gho_")
                    || word.starts_with("ghu_")
                    || word.starts_with("ghs_")
                    || word.starts_with("ghr_")
                    || word.starts_with("github_pat_"))
                    && word.len() >= 30)
                    || ((word.starts_with("sk-") || word.starts_with("sk_")) && word.len() >= 20)
                    || (word.starts_with("AKIA") && word.len() == 20)
            })
}

/// Transactional control-plane failure.
#[derive(Debug, Error)]
pub enum StoreError {
    /// `SQLite` operation failed.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Brief or persisted typed value failed validation.
    #[error("brief validation error: {0}")]
    Brief(#[from] BriefError),
    /// JSON evidence failed serialization.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Database was created by a newer incompatible binary.
    #[error("unsupported control-plane schema version: {0}")]
    UnsupportedSchema(i64),
    /// Unit id does not exist.
    #[error("unknown unit: {0}")]
    UnknownUnit(String),
    /// State edge violates the declared lifecycle.
    #[error("invalid transition from {current:?} to {next:?}")]
    InvalidTransition {
        /// Current persisted state.
        current: JobState,
        /// Refused target state.
        next: JobState,
    },
    /// Evidence-bearing state requires a dedicated API.
    #[error("state {0:?} requires its dedicated evidence gate")]
    DedicatedGateRequired(JobState),
    /// Candidate must always be a committed full SHA.
    #[error("candidate SHA is missing")]
    MissingCandidate,
    /// Verifier checked a different content revision.
    #[error("verification SHA {verdict} is stale; candidate is {candidate}")]
    StaleVerification {
        /// Current candidate.
        candidate: Sha,
        /// Refused verdict revision.
        verdict: Sha,
    },
    /// No exact current passing verdict exists.
    #[error("no passing verification verdict for exact SHA {0}")]
    MissingPassingVerdict(Sha),
    /// Merge request does not match the current verified integration.
    #[error("requested SHA {actual} does not match authorized SHA {expected}")]
    UnauthorizedSha {
        /// Current verified or authorized SHA.
        expected: Sha,
        /// Refused requested SHA.
        actual: Sha,
    },
    /// Merge authorization requires a live same-job shipper profile.
    #[error("profile is not an authorized live shipper")]
    UnauthorizedShipper,
    /// Shipper attempted merge without a recorded capability grant.
    #[error("merge authorization is missing")]
    MissingMergeAuthorization,
    /// Lease request is empty or already expired.
    #[error("invalid lease request")]
    InvalidLease,
    /// Another live worker owns an overlapping resource.
    #[error("live lease overlaps resource: {0}")]
    LeaseConflict(String),
    /// Worker output arrived after its authority expired.
    #[error("worker result belongs to stale lease {0}")]
    StaleLease(i64),
    /// Append-only event keys and types are required.
    #[error("event idempotency key and type must not be empty")]
    InvalidEvent,
    /// An idempotency key was reused for a different logical event.
    #[error("idempotency key was reused with different evidence: {0}")]
    IdempotencyConflict(String),
    /// A durable command result could not be decoded as its advertised type.
    #[error("stored command result is invalid: {0}")]
    InvalidStoredCommand(String),
    /// Profile home must be an explicit isolated absolute path.
    #[error("profile home must be absolute: {path}", path = .0.display())]
    InvalidProfileHome(std::path::PathBuf),
    /// Profile registration cannot cross job ownership.
    #[error("job and unit ownership do not match")]
    JobUnitMismatch,
    /// Existing job identity cannot be reused for a different repository or policy pin.
    #[error("job scope differs from its immutable definition: {0}")]
    JobScopeMismatch(String),
    /// Protected integration recovery requires an explicitly supported edge.
    #[error("cannot recover unit from {current:?} to {next:?}")]
    InvalidRecovery {
        /// Current protected state.
        current: JobState,
        /// Requested recovery state.
        next: JobState,
    },
    /// Recovery actors must own the capability associated with the state.
    #[error("profile is not authorized to recover the protected integration state")]
    UnauthorizedRecovery,
    /// A mutation actor must be live, exactly scoped, and hold the required capability.
    #[error("profile is not authorized for this unit mutation: {0}")]
    UnauthorizedActor(String),
    /// A mutation actor must own the required current lease for the exact unit.
    #[error("profile {actor} lacks a live {kind:?} lease for this unit")]
    MissingActorLease {
        /// Actor whose lease was required.
        actor: String,
        /// Required lease category.
        kind: LeaseKind,
    },
    /// Every dependency must name an already persisted unit.
    #[error("unknown dependency unit: {0}")]
    UnknownDependency(String),
    /// A job-level credential identifier cannot change methods between units.
    #[error("credential grant methods differ within the job: {0}")]
    CredentialGrantConflict(String),
    /// A worker cannot lease a unit before every declared dependency merges.
    #[error("unit has {0} unsatisfied dependencies")]
    DependenciesUnsatisfied(i64),
    /// Session keys are always explicit.
    #[error("session external key must not be empty")]
    InvalidSessionKey,
    /// Profile is unknown or its authority has already been destroyed.
    #[error("profile is not live: {0}")]
    UnknownLiveProfile(String),
    /// Worktree must be an explicit scheduler-owned absolute path.
    #[error("worktree must be absolute: {path}", path = .0.display())]
    InvalidWorktree(std::path::PathBuf),
    /// Branch namespace and base must derive from the immutable brief.
    #[error("branch name or base SHA does not match the unit assignment")]
    InvalidBranchAssignment,
    /// A unit has no scheduler-registered coding branch.
    #[error("unit has no registered branch: {0}")]
    UnknownBranch(String),
    /// Branch commits are accepted only during active coding states.
    #[error("branch head cannot change while unit is {0:?}")]
    BranchUpdateOutsideCodingState(JobState),
    /// Branch update or candidate report was based on a stale head.
    #[error("branch head is {current}; caller expected {expected}")]
    StaleBranchHead {
        /// Current scheduler-tracked head.
        current: Sha,
        /// Head asserted by the caller.
        expected: Sha,
    },
    /// Only the same-job live coordinator may revoke a credential grant.
    #[error("profile is not an authorized live coordinator")]
    UnauthorizedCoordinator,
    /// The requested grant does not exist or was already revoked.
    #[error("credential grant is not active: {0}")]
    UnknownActiveCredentialGrant(String),
    /// Artifact paths are safe repository-relative evidence locations.
    #[error("artifact path must be safe and repository-relative")]
    InvalidArtifact,
    /// Artifact evidence belongs to a different source revision.
    #[error("artifact SHA {artifact} is stale; current unit SHA is {current}")]
    StaleArtifact {
        /// Current base or candidate revision.
        current: Sha,
        /// Revision asserted by the artifact.
        artifact: Sha,
    },
    /// Reviews are accepted only during the explicit review state.
    #[error("review was submitted outside the reviewing state")]
    ReviewOutsideReviewState,
    /// Only an isolated verifier/reviewer profile for the same job may review.
    #[error("profile is not an authorized independent reviewer")]
    UnauthorizedReviewer,
    /// Only a live same-job verifier capability may publish a verdict.
    #[error("profile is not an authorized independent verifier")]
    UnauthorizedVerifier,
    /// Only a live same-job integrator may dispose review findings.
    #[error("profile is not an authorized live integrator")]
    UnauthorizedIntegrator,
    /// Finding is stale, unknown, or already has a final disposition.
    #[error("review finding is not open: {0}")]
    UnknownOpenFinding(i64),
    /// Medium/high-risk changes need two independent non-blocking reviews.
    #[error(
        "review gate needs two reviewers, dispositions, and no blockers; reviewers={reviewers}, unresolved={unresolved}, blocking={blocking}"
    )]
    ReviewGateUnsatisfied {
        /// Distinct independent reviewer profiles observed.
        reviewers: i64,
        /// Findings still awaiting integrator disposition.
        unresolved: i64,
        /// Remaining blocking assessments.
        blocking: i64,
    },
    /// Worker report did not satisfy the immutable brief's required object fields.
    #[error("worker report does not satisfy the immutable report schema")]
    ReportSchemaViolation,
}

const SCHEMA: &str = r"
CREATE TABLE jobs (
    job_id TEXT PRIMARY KEY,
    brief_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE units (
    unit_id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(job_id),
    state TEXT NOT NULL,
    base_sha TEXT NOT NULL,
    candidate_sha TEXT,
    integration_sha TEXT,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE dependencies (
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    depends_on_unit_id TEXT NOT NULL,
    PRIMARY KEY (unit_id, depends_on_unit_id)
) STRICT;

CREATE TABLE profiles (
    profile_id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(job_id),
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    role TEXT NOT NULL,
    home TEXT NOT NULL UNIQUE,
    destroyed_at INTEGER
) STRICT;

CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(profile_id),
    external_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (profile_id, external_key)
) STRICT;

CREATE TABLE leases (
    lease_id INTEGER PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(job_id),
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    kind TEXT NOT NULL,
    resource TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    released_at INTEGER
) STRICT;

CREATE TABLE credential_grants (
    grant_id INTEGER PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(job_id),
    credential_id TEXT NOT NULL,
    methods_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER
) STRICT;

CREATE TABLE branches (
    branch_id INTEGER PRIMARY KEY,
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    name TEXT NOT NULL UNIQUE,
    worktree TEXT NOT NULL UNIQUE,
    base_sha TEXT NOT NULL,
    head_sha TEXT
) STRICT;

CREATE TABLE events (
    event_id INTEGER PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(job_id),
    idempotency_key TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE artifacts (
    artifact_id INTEGER PRIMARY KEY,
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    digest TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (unit_id, path, digest)
) STRICT;

CREATE TABLE verification_verdicts (
    verdict_id INTEGER PRIMARY KEY,
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    head_sha TEXT NOT NULL,
    passed INTEGER NOT NULL CHECK (passed IN (0, 1)),
    evidence_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE review_findings (
    finding_id INTEGER PRIMARY KEY,
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    head_sha TEXT NOT NULL,
    severity TEXT NOT NULL,
    finding_json TEXT NOT NULL,
    disposition TEXT,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE merge_authorizations (
    authorization_id INTEGER PRIMARY KEY,
    unit_id TEXT NOT NULL UNIQUE REFERENCES units(unit_id),
    head_sha TEXT NOT NULL,
    authorized_by TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TRIGGER jobs_brief_immutable
BEFORE UPDATE OF brief_json ON jobs
BEGIN
    SELECT RAISE(ABORT, 'job brief is immutable');
END;

CREATE TRIGGER events_no_update
BEFORE UPDATE ON events
BEGIN
    SELECT RAISE(ABORT, 'events are append-only');
END;

CREATE TRIGGER events_no_delete
BEFORE DELETE ON events
BEGIN
    SELECT RAISE(ABORT, 'events are append-only');
END;
";

const MIGRATION_2: &str = r"
ALTER TABLE review_findings
ADD COLUMN reviewer_profile TEXT NOT NULL DEFAULT 'legacy-reviewer';
";

const MIGRATION_3: &str = r"
ALTER TABLE sessions
ADD COLUMN destroyed_at INTEGER;
";

const MIGRATION_4: &str = r"
ALTER TABLE artifacts RENAME TO artifacts_v3;

CREATE TABLE artifacts (
    artifact_id INTEGER PRIMARY KEY,
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    digest TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    head_sha TEXT,
    UNIQUE (unit_id, path, digest, head_sha)
) STRICT;

INSERT INTO artifacts (artifact_id, unit_id, kind, path, digest, created_at)
SELECT artifact_id, unit_id, kind, path, digest, created_at FROM artifacts_v3;

DROP TABLE artifacts_v3;
";

const MIGRATION_5: &str = r"
ALTER TABLE verification_verdicts
ADD COLUMN verifier_profile TEXT REFERENCES profiles(profile_id);

CREATE UNIQUE INDEX verification_verdict_actor_sha
ON verification_verdicts (unit_id, head_sha, verifier_profile)
WHERE verifier_profile IS NOT NULL;
";

const MIGRATION_6: &str = r"
ALTER TABLE jobs ADD COLUMN repository TEXT;
ALTER TABLE jobs ADD COLUMN standing_policy_version TEXT;

UPDATE jobs
SET repository = json_extract(brief_json, '$.repository'),
    standing_policy_version = json_extract(brief_json, '$.standing_policy_version');

CREATE TABLE unit_briefs (
    unit_id TEXT PRIMARY KEY REFERENCES units(unit_id),
    brief_json TEXT NOT NULL
) STRICT;

INSERT INTO unit_briefs (unit_id, brief_json)
SELECT units.unit_id, jobs.brief_json
FROM units JOIN jobs USING (job_id);

DROP TRIGGER jobs_brief_immutable;
ALTER TABLE jobs DROP COLUMN brief_json;

ALTER TABLE dependencies RENAME TO dependencies_v5;

CREATE TABLE dependencies (
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    depends_on_unit_id TEXT NOT NULL REFERENCES units(unit_id),
    PRIMARY KEY (unit_id, depends_on_unit_id)
) STRICT;

CREATE TRIGGER dependencies_same_job
BEFORE INSERT ON dependencies
WHEN (SELECT job_id FROM units WHERE unit_id = NEW.unit_id)
   != (SELECT job_id FROM units WHERE unit_id = NEW.depends_on_unit_id)
BEGIN
    SELECT RAISE(ABORT, 'dependency must belong to the same job');
END;

INSERT INTO dependencies (unit_id, depends_on_unit_id)
SELECT unit_id, depends_on_unit_id FROM dependencies_v5;

DROP TABLE dependencies_v5;

CREATE UNIQUE INDEX credential_grants_job_credential
ON credential_grants (job_id, credential_id);

CREATE TRIGGER jobs_scope_immutable
BEFORE UPDATE OF repository, standing_policy_version ON jobs
BEGIN
    SELECT RAISE(ABORT, 'job scope is immutable');
END;

CREATE TRIGGER jobs_scope_required
BEFORE INSERT ON jobs
WHEN NEW.repository IS NULL OR trim(NEW.repository) = ''
  OR NEW.standing_policy_version IS NULL OR trim(NEW.standing_policy_version) = ''
BEGIN
    SELECT RAISE(ABORT, 'job scope is required');
END;

CREATE TRIGGER unit_briefs_immutable
BEFORE UPDATE OF brief_json ON unit_briefs
BEGIN
    SELECT RAISE(ABORT, 'unit brief is immutable');
END;
";

const MIGRATION_7: &str = r"
ALTER TABLE merge_authorizations RENAME TO merge_authorizations_v6;

CREATE TABLE merge_authorizations (
    authorization_id INTEGER PRIMARY KEY,
    unit_id TEXT NOT NULL REFERENCES units(unit_id),
    head_sha TEXT NOT NULL,
    authorized_by TEXT NOT NULL REFERENCES profiles(profile_id),
    created_at INTEGER NOT NULL,
    invalidated_at INTEGER
) STRICT;

INSERT INTO merge_authorizations (
    authorization_id, unit_id, head_sha, authorized_by, created_at
)
SELECT authorization_id, unit_id, head_sha, authorized_by, created_at
FROM merge_authorizations_v6;

DROP TABLE merge_authorizations_v6;

CREATE UNIQUE INDEX merge_authorizations_one_active
ON merge_authorizations (unit_id)
WHERE invalidated_at IS NULL;

CREATE TABLE command_results (
    idempotency_key TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(job_id),
    command_type TEXT NOT NULL,
    request_json TEXT NOT NULL,
    result_json TEXT NOT NULL,
    event_id INTEGER NOT NULL UNIQUE REFERENCES events(event_id),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TRIGGER command_results_no_update
BEFORE UPDATE ON command_results
BEGIN
    SELECT RAISE(ABORT, 'command results are immutable');
END;

CREATE TRIGGER command_results_no_delete
BEFORE DELETE ON command_results
BEGIN
    SELECT RAISE(ABORT, 'command results are immutable');
END;
";

const MIGRATION_8: &str = r"
ALTER TABLE leases
ADD COLUMN holder_profile TEXT REFERENCES profiles(profile_id);

CREATE TRIGGER leases_holder_required_and_scoped
BEFORE INSERT ON leases
WHEN NEW.holder_profile IS NULL
  OR NOT EXISTS (
      SELECT 1 FROM profiles
      WHERE profile_id = NEW.holder_profile
        AND job_id = NEW.job_id
        AND unit_id = NEW.unit_id
        AND destroyed_at IS NULL
  )
BEGIN
    SELECT RAISE(ABORT, 'lease holder must be a live profile in the exact job and unit');
END;

CREATE TRIGGER leases_holder_immutable
BEFORE UPDATE OF holder_profile ON leases
BEGIN
    SELECT RAISE(ABORT, 'lease holder is immutable');
END;
";

const MIGRATION_9: &str = r"
CREATE TRIGGER verification_verdicts_no_update
BEFORE UPDATE ON verification_verdicts
BEGIN
    SELECT RAISE(ABORT, 'verification verdicts are immutable');
END;

CREATE TRIGGER verification_verdicts_no_delete
BEFORE DELETE ON verification_verdicts
BEGIN
    SELECT RAISE(ABORT, 'verification verdicts are immutable');
END;
";

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rusqlite::OptionalExtension;
    use serde_json::json;

    use super::{
        ControlStore, FindingDisposition, MIGRATION_2, MIGRATION_3, MIGRATION_4, MIGRATION_5,
        MIGRATION_6, MIGRATION_7, MIGRATION_8, ReviewAssessment, SCHEMA, StoreError,
        VerificationVerdict, command_replay_tx, contains_secret_shape, is_safe_relative_path,
        redact_evidence,
    };
    use crate::{
        ArtifactKind, BriefError, CredentialGrant, JobBrief, JobId, JobState, LeaseKind,
        NetworkMode, NetworkPolicy, PathPolicy, ProfileId, ResourceLimits, RiskClass, Role,
        SessionId, Sha, UnitId, VerificationCommand,
    };

    fn sha(character: char) -> Sha {
        Sha::new(character.to_string().repeat(40)).expect("valid SHA")
    }

    fn brief() -> JobBrief {
        JobBrief {
            job_id: JobId::new("job-1").expect("valid job"),
            unit_id: UnitId::new("unit-1").expect("valid unit"),
            goal: "Implement a bounded unit and prove it".to_owned(),
            repository: "https://github.com/nickderaj/nswarm".to_owned(),
            base_sha: sha('a'),
            paths: PathPolicy {
                readable: vec![PathBuf::from("crates/assigned")],
                writable: vec![PathBuf::from("crates/assigned/src")],
                forbidden: vec![PathBuf::from("crates/sibling")],
            },
            dependencies: Vec::new(),
            acceptance_criteria: vec!["focused test passes".to_owned()],
            verification_commands: vec![VerificationCommand {
                program: "cargo".to_owned(),
                arguments: vec!["test".to_owned(), "-p".to_owned(), "assigned".to_owned()],
            }],
            risk_class: RiskClass::Medium,
            limits: ResourceLimits {
                wall_seconds: 900,
                memory_bytes: 1_000_000_000,
                disk_bytes: 1_000_000_000,
                process_count: 64,
                cost_microunits: 0,
            },
            network: NetworkPolicy {
                mode: NetworkMode::DenyAll,
                destinations: Vec::new(),
            },
            credential_grants: vec![CredentialGrant {
                credential_id: "github-job-push".to_owned(),
                methods: vec!["git:push:refs/heads/nswarm/job-1/unit-1".to_owned()],
            }],
            report_schema: json!({
                "type": "object",
                "required": ["head_sha", "evidence"],
                "additionalProperties": false,
                "properties": {
                    "head_sha": {"type": "string"},
                    "evidence": {
                        "type": "object",
                        "required": ["checks"],
                        "additionalProperties": false,
                        "properties": {
                            "checks": {
                                "type": "array",
                                "items": {"type": "string"}
                            },
                            "details": {
                                "type": "object",
                                "properties": {},
                                "additionalProperties": true
                            }
                        }
                    }
                }
            }),
            standing_policy_version: "v1".to_owned(),
        }
    }

    fn ensure_profile(
        store: &mut ControlStore,
        brief: &JobBrief,
        label: &str,
        role: Role,
        now: i64,
    ) -> ProfileId {
        let profile = ProfileId::new(label).expect("profile id");
        let exists: bool = store
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM profiles WHERE profile_id = ?1)",
                [profile.as_str()],
                |row| row.get(0),
            )
            .expect("profile lookup");
        if !exists {
            store
                .register_profile(
                    &profile,
                    &brief.job_id,
                    &brief.unit_id,
                    role,
                    PathBuf::from(format!("/tmp/nswarm-{label}")).as_path(),
                    now,
                )
                .expect("profile registered");
        }
        profile
    }

    fn ensure_coordinator(store: &mut ControlStore, brief: &JobBrief, now: i64) -> ProfileId {
        ensure_profile(
            store,
            brief,
            &format!("coordinator-{}-{}", brief.job_id, brief.unit_id),
            Role::Coordinator,
            now,
        )
    }

    fn ensure_profile_lease(
        store: &mut ControlStore,
        brief: &JobBrief,
        holder: &ProfileId,
        now: i64,
    ) -> i64 {
        let existing: Option<i64> = store
            .connection
            .query_row(
                "SELECT lease_id FROM leases WHERE holder_profile = ?1 AND unit_id = ?2 AND kind = 'profile' AND released_at IS NULL AND expires_at > ?3",
                rusqlite::params![holder.as_str(), brief.unit_id.as_str(), now],
                |row| row.get(0),
            )
            .optional()
            .expect("lease lookup");
        if let Some(lease_id) = existing {
            return lease_id;
        }
        let coordinator = ensure_coordinator(store, brief, now);
        store
            .acquire_lease(
                &coordinator,
                holder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                holder.as_str(),
                10_000,
                now,
            )
            .expect("profile lease acquired")
    }

    fn ensure_integrator(store: &mut ControlStore, brief: &JobBrief, now: i64) -> ProfileId {
        let integrator = ensure_profile(
            store,
            brief,
            &format!("integrator-{}-{}", brief.job_id, brief.unit_id),
            Role::Integrator,
            now,
        );
        ensure_profile_lease(store, brief, &integrator, now);
        integrator
    }

    fn ensure_topology_lease(
        store: &mut ControlStore,
        brief: &JobBrief,
        integrator: &ProfileId,
        now: i64,
    ) -> i64 {
        let coordinator = ensure_coordinator(store, brief, now);
        store
            .acquire_lease(
                &coordinator,
                integrator,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Topology,
                "integration",
                10_000,
                now,
            )
            .expect("topology lease acquired")
    }

    fn advance_to_self_verifying(store: &mut ControlStore, brief: &JobBrief) -> ProfileId {
        let coordinator = ensure_coordinator(store, brief, 2);
        let coder = ensure_profile(
            store,
            brief,
            &format!("coder-{}-{}", brief.job_id, brief.unit_id),
            Role::Coder,
            2,
        );
        store
            .transition(
                &coordinator,
                &brief.unit_id,
                JobState::Leased,
                "advance-2",
                2,
            )
            .expect("unit leased");
        ensure_profile_lease(store, brief, &coder, 2);
        for (state, timestamp) in [
            JobState::Grounding,
            JobState::Implementing,
            JobState::SelfVerifying,
        ]
        .into_iter()
        .zip([3_i64, 4, 5])
        {
            store
                .transition(
                    &coder,
                    &brief.unit_id,
                    state,
                    &format!("advance-{timestamp}"),
                    timestamp,
                )
                .expect("valid transition");
        }
        coder
    }

    fn register_verifier(
        store: &mut ControlStore,
        brief: &JobBrief,
        label: &str,
        now: i64,
    ) -> ProfileId {
        let verifier = ProfileId::new(format!("verifier-{label}")).expect("verifier id");
        store
            .register_profile(
                &verifier,
                &brief.job_id,
                &brief.unit_id,
                Role::VerifierReviewer,
                PathBuf::from(format!("/tmp/nswarm-verifier-{label}")).as_path(),
                now,
            )
            .expect("verifier registered");
        ensure_profile_lease(store, brief, &verifier, now);
        verifier
    }

    fn prepare_reviewing_candidate(store: &mut ControlStore, brief: &JobBrief, candidate: &Sha) {
        store.create_job(brief, 1).expect("job created");
        let coder = advance_to_self_verifying(store, brief);
        store
            .record_candidate(&coder, &brief.unit_id, candidate, "candidate-prepared", 7)
            .expect("candidate recorded");
        let verifier = register_verifier(store, brief, "prepared", 8);
        store
            .transition(
                &verifier,
                &brief.unit_id,
                JobState::IndependentlyVerifying,
                "verification-prepared",
                8,
            )
            .expect("verification starts");
        let evidence = json!({"commands": ["cargo test"]});
        let verdict = VerificationVerdict {
            verifier: &verifier,
            head_sha: candidate,
            passed: true,
            evidence: &evidence,
            idempotency_key: "verdict-prepared",
        };
        store
            .record_verdict(&brief.unit_id, &verdict, 9)
            .expect("verdict recorded");
        store
            .record_verdict(&brief.unit_id, &verdict, 99)
            .expect("verdict replayed after state advance");
    }

    fn record_two_reviews(store: &mut ControlStore, brief: &JobBrief, head_sha: &Sha, now: i64) {
        let integrator = ensure_integrator(store, brief, now);
        for index in 1..=2 {
            let profile = ProfileId::new(format!("reviewer-{index}")).expect("valid profile");
            store
                .register_profile(
                    &profile,
                    &brief.job_id,
                    &brief.unit_id,
                    Role::VerifierReviewer,
                    PathBuf::from(format!("/tmp/nswarm-reviewer-{index}")).as_path(),
                    now + index,
                )
                .expect("review profile registered");
            ensure_profile_lease(store, brief, &profile, now + index);
            let finding_id = store
                .record_review(
                    &brief.unit_id,
                    &profile,
                    head_sha,
                    ReviewAssessment::Noted,
                    &json!({"summary": "independent review passed"}),
                    now + index + 2,
                )
                .expect("review recorded");
            let rationale = json!({"reason": "independent review evidence accepted"});
            let idempotency_key = format!("review-disposed:{finding_id}");
            let disposition = FindingDisposition {
                finding_id,
                disposition: ReviewAssessment::Noted,
                rationale: &rationale,
                idempotency_key: &idempotency_key,
            };
            store
                .dispose_review_finding(&brief.unit_id, &integrator, &disposition, now + index + 4)
                .expect("review disposed");
            store
                .dispose_review_finding(&brief.unit_id, &integrator, &disposition, 99)
                .expect("review disposition replayed after mutation");
        }
    }

    fn register_shipper(store: &mut ControlStore, brief: &JobBrief, now: i64) -> ProfileId {
        let shipper = ProfileId::new("shipper-job-1").expect("shipper id");
        store
            .register_profile(
                &shipper,
                &brief.job_id,
                &brief.unit_id,
                Role::Shipper,
                std::path::Path::new("/tmp/nswarm-shipper-job-1"),
                now,
            )
            .expect("shipper profile registered");
        shipper
    }

    fn record_unresolved_reviews(
        store: &mut ControlStore,
        brief: &JobBrief,
        candidate: &Sha,
    ) -> (Vec<ProfileId>, Vec<i64>) {
        let mut findings = Vec::new();
        let mut reviewers = Vec::new();
        for index in 1..=2 {
            let reviewer =
                ProfileId::new(format!("disposition-reviewer-{index}")).expect("reviewer id");
            store
                .register_profile(
                    &reviewer,
                    &brief.job_id,
                    &brief.unit_id,
                    Role::VerifierReviewer,
                    PathBuf::from(format!("/tmp/nswarm-disposition-reviewer-{index}")).as_path(),
                    10 + index,
                )
                .expect("reviewer registered");
            ensure_profile_lease(store, brief, &reviewer, 10 + index);
            findings.push(
                store
                    .record_review(
                        &brief.unit_id,
                        &reviewer,
                        candidate,
                        ReviewAssessment::Consider,
                        &json!({"summary": "non-blocking concern"}),
                        12 + index,
                    )
                    .expect("review recorded"),
            );
            reviewers.push(reviewer);
        }
        (reviewers, findings)
    }

    #[test]
    fn migration_is_idempotent() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        store.migrate().expect("second migration succeeds");
    }

    #[test]
    fn completed_commands_replay_before_state_checks_without_duplicate_effects() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ensure_coordinator(&mut store, &brief, 2);
        let coder = ensure_profile(&mut store, &brief, "coder-job-1", Role::Coder, 2);

        store
            .transition(&coordinator, &unit, JobState::Leased, "lease-command", 2)
            .expect("initial transition succeeds");
        store
            .transition(&coordinator, &unit, JobState::Leased, "lease-command", 99)
            .expect("identical retry succeeds after state advance");
        assert!(matches!(
            store.transition(&coder, &unit, JobState::Grounding, "lease-command", 3),
            Err(StoreError::IdempotencyConflict(key)) if key == "lease-command"
        ));
        ensure_profile_lease(&mut store, &brief, &coder, 3);
        store
            .transition(&coder, &unit, JobState::Grounding, "ground-command", 4)
            .expect("later command advances state");
        let unknown = ProfileId::new("unknown-recovery-actor").expect("profile id");
        assert!(matches!(
            store.recover_integration(
                &unit,
                &unknown,
                JobState::Merged,
                &json!({"reason": "not a recovery target"}),
                "invalid-recovery-target",
                5,
            ),
            Err(StoreError::InvalidRecovery { .. })
        ));
        assert!(matches!(
            store.recover_integration(
                &unit,
                &unknown,
                JobState::FixRequired,
                &json!({"reason": "wrong origin state"}),
                "invalid-recovery-origin",
                6,
            ),
            Err(StoreError::InvalidRecovery { .. })
        ));
        store
            .transition(&coordinator, &unit, JobState::Leased, "lease-command", 100)
            .expect("original result remains replayable after further progress");

        let (events, commands): (i64, i64) = store
            .connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM events WHERE idempotency_key = 'lease-command'), (SELECT COUNT(*) FROM command_results WHERE idempotency_key = 'lease-command')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("single-effect query");
        assert_eq!((events, commands), (1, 1));
    }

    #[test]
    fn command_replay_rejects_each_blank_identity_field() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let transaction = store.connection.transaction().expect("transaction");
        for (key, command_type) in [("", "transition"), ("command-key", " ")] {
            assert!(matches!(
                command_replay_tx(&transaction, key, command_type, &json!({})),
                Err(StoreError::InvalidEvent)
            ));
        }
    }

    #[test]
    fn migration_refuses_a_newer_unknown_schema() {
        let connection = rusqlite::Connection::open_in_memory().expect("connection opens");
        connection
            .pragma_update(None, "user_version", super::SCHEMA_VERSION + 1)
            .expect("future version set");
        let mut store = ControlStore { connection };
        assert!(matches!(
            store.migrate(),
            Err(StoreError::UnsupportedSchema(version))
                if version == super::SCHEMA_VERSION + 1
        ));
    }

    #[test]
    fn missing_brief_fields_refuse_creation() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let mut invalid = brief();
        invalid.verification_commands.clear();
        assert!(store.create_job(&invalid, 1).is_err());
        let mut invalid = brief();
        invalid.report_schema = json!({
            "type": "object",
            "required": ["head_sha"],
            "properties": {"head_sha": {"type": "executable"}}
        });
        assert!(matches!(
            store.create_job(&invalid, 2),
            Err(StoreError::Brief(BriefError::InvalidReportSchema))
        ));
    }

    #[test]
    fn candidate_requires_commit_sha_and_independent_proof() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        store.create_job(&brief, 1).expect("job created");
        let unknown = ProfileId::new("unknown-candidate-actor").expect("profile id");
        assert!(matches!(
            store.transition(&unknown, &unit, JobState::Verified, "skip", 2),
            Err(StoreError::DedicatedGateRequired(JobState::Verified))
        ));
        let coder = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(&coder, &unit, &sha('b'), "candidate", 7)
            .expect("candidate recorded");
        assert_eq!(store.state(&unit).expect("state"), JobState::CandidateReady);
    }

    #[test]
    fn eval_exact_sha_corpus_invalidates_stale_verification() {
        let case: serde_json::Value =
            serde_json::from_str(include_str!("../../../eval/corpus/exact-sha.json"))
                .expect("exact-SHA eval corpus parses");
        let candidate = Sha::new(
            case["input"]["candidate_sha"]
                .as_str()
                .expect("candidate SHA is text"),
        )
        .expect("candidate is a full SHA");
        let stale = Sha::new(
            case["input"]["stale_sha"]
                .as_str()
                .expect("stale SHA is text"),
        )
        .expect("stale value is a full SHA");
        assert_eq!(
            Sha::new(
                case["input"]["abbreviated_sha"]
                    .as_str()
                    .expect("abbreviated SHA is text")
            )
            .is_ok(),
            case["expected"]["abbreviated_sha_accepted"]
                .as_bool()
                .expect("abbreviated expectation is a boolean")
        );
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        store.create_job(&brief, 1).expect("job created");
        let coder = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(&coder, &unit, &candidate, "candidate", 7)
            .expect("candidate recorded");
        let verifier = register_verifier(&mut store, &brief, "changed-sha", 8);
        store
            .transition(
                &verifier,
                &unit,
                JobState::IndependentlyVerifying,
                "verify",
                8,
            )
            .expect("verification starts");
        let stale_accepted = store
            .record_verdict(
                &unit,
                &VerificationVerdict {
                    verifier: &verifier,
                    head_sha: &stale,
                    passed: true,
                    evidence: &json!({}),
                    idempotency_key: "stale",
                },
                9,
            )
            .is_ok();
        assert_eq!(
            stale_accepted,
            case["expected"]["stale_verdict_accepted"]
                .as_bool()
                .expect("stale verdict expectation is a boolean")
        );
    }

    #[test]
    fn integration_content_change_requires_reverification() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        store.create_job(&brief, 1).expect("job created");
        let coder = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(&coder, &unit, &sha('b'), "candidate", 7)
            .expect("candidate recorded");
        let verifier = register_verifier(&mut store, &brief, "integration", 8);
        store
            .transition(
                &verifier,
                &unit,
                JobState::IndependentlyVerifying,
                "verify",
                8,
            )
            .expect("verification starts");
        store
            .record_verdict(
                &unit,
                &VerificationVerdict {
                    verifier: &verifier,
                    head_sha: &sha('b'),
                    passed: true,
                    evidence: &json!({"commands": ["cargo test"]}),
                    idempotency_key: "verdict",
                },
                9,
            )
            .expect("verdict recorded");
        let integrator = ensure_integrator(&mut store, &brief, 10);
        assert!(matches!(
            store.accept_verdict(&integrator, &unit, "premature-accept", 10),
            Err(StoreError::ReviewGateUnsatisfied { reviewers: 0, .. })
        ));
        record_two_reviews(&mut store, &brief, &sha('b'), 10);
        assert_eq!(
            store
                .accept_verdict(&integrator, &unit, "accept", 15)
                .expect("accepted"),
            JobState::Verified
        );
        ensure_topology_lease(&mut store, &brief, &integrator, 16);
        store
            .transition(&integrator, &unit, JobState::Integrating, "integrate", 16)
            .expect("integration starts");
        assert_eq!(
            store
                .complete_integration(&integrator, &unit, &sha('c'), "integrated", 17)
                .expect("integration completes"),
            JobState::CandidateReady
        );
        assert!(matches!(
            store.authorize_merge(
                &unit,
                &sha('c'),
                &ProfileId::new("shipper-job-1").expect("shipper id"),
                "authorize",
                18,
            ),
            Err(StoreError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn verification_guards_reject_wrong_states_and_failed_evidence() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        let candidate = sha('b');
        store.create_job(&brief, 1).expect("job created");
        let unknown = ProfileId::new("unknown-verifier").expect("profile id");
        assert!(matches!(
            store.record_verdict(
                &unit,
                &VerificationVerdict {
                    verifier: &unknown,
                    head_sha: &candidate,
                    passed: true,
                    evidence: &json!({}),
                    idempotency_key: "too-early",
                },
                2,
            ),
            Err(StoreError::InvalidTransition { .. })
        ));
        assert!(matches!(
            store.accept_verdict(&unknown, &unit, "too-early-accept", 2),
            Err(StoreError::InvalidTransition { .. })
        ));
        let coder = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(&coder, &unit, &candidate, "failed-candidate", 7)
            .expect("candidate recorded");
        let verifier = register_verifier(&mut store, &brief, "failed", 8);
        store
            .transition(
                &verifier,
                &unit,
                JobState::IndependentlyVerifying,
                "failed-verification-started",
                8,
            )
            .expect("verification starts");
        store
            .record_verdict(
                &unit,
                &VerificationVerdict {
                    verifier: &verifier,
                    head_sha: &candidate,
                    passed: false,
                    evidence: &json!({"result": "failed"}),
                    idempotency_key: "failed-verdict",
                },
                9,
            )
            .expect("failing verdict is durable");
        let integrator = ensure_integrator(&mut store, &brief, 10);
        assert!(matches!(
            store.accept_verdict(&integrator, &unit, "failed-accept", 10),
            Err(StoreError::MissingPassingVerdict(head)) if head == candidate
        ));

        let reviewer = ProfileId::new("stale-reviewer").expect("reviewer id");
        store
            .register_profile(
                &reviewer,
                &brief.job_id,
                &unit,
                Role::VerifierReviewer,
                std::path::Path::new("/tmp/nswarm-stale-reviewer"),
                10,
            )
            .expect("reviewer registered");
        ensure_profile_lease(&mut store, &brief, &reviewer, 10);
        assert!(matches!(
            store.record_review(
                &unit,
                &reviewer,
                &sha('c'),
                ReviewAssessment::Noted,
                &json!({}),
                11,
            ),
            Err(StoreError::StaleVerification { .. })
        ));
    }

    #[test]
    fn verdict_requires_a_live_verifier_capability() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let candidate = sha('b');
        store.create_job(&brief, 1).expect("job created");
        let coding_actor = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(
                &coding_actor,
                &brief.unit_id,
                &candidate,
                "auth-candidate",
                7,
            )
            .expect("candidate recorded");
        let gate_verifier = register_verifier(&mut store, &brief, "gate", 8);
        store
            .transition(
                &gate_verifier,
                &brief.unit_id,
                JobState::IndependentlyVerifying,
                "auth-verification-started",
                8,
            )
            .expect("verification starts");

        let unknown = ProfileId::new("unknown-verifier").expect("profile id");
        let coder = ProfileId::new("coder-verdict").expect("coder id");
        store
            .register_profile(
                &coder,
                &brief.job_id,
                &brief.unit_id,
                Role::Coder,
                std::path::Path::new("/tmp/nswarm-coder-verdict"),
                8,
            )
            .expect("coder registered");
        for (profile, key) in [(&unknown, "unknown-verdict"), (&coder, "coder-verdict")] {
            assert!(matches!(
                store.record_verdict(
                    &brief.unit_id,
                    &VerificationVerdict {
                        verifier: profile,
                        head_sha: &candidate,
                        passed: true,
                        evidence: &json!({}),
                        idempotency_key: key,
                    },
                    8,
                ),
                Err(StoreError::UnauthorizedVerifier)
            ));
        }
    }

    #[test]
    fn any_attributed_failure_blocks_the_exact_sha() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let candidate = sha('b');
        prepare_reviewing_candidate(&mut store, &brief, &candidate);
        let dissenting = register_verifier(&mut store, &brief, "dissenting", 10);
        store
            .connection
            .execute(
                "INSERT INTO verification_verdicts (unit_id, verifier_profile, head_sha, passed, evidence_json, created_at) VALUES (?1, ?2, ?3, 0, '{}', 10)",
                rusqlite::params![
                    brief.unit_id.as_str(),
                    dissenting.as_str(),
                    candidate.as_str()
                ],
            )
            .expect("independent failure recorded");
        let integrator = ensure_integrator(&mut store, &brief, 11);
        assert!(matches!(
            store.accept_verdict(&integrator, &brief.unit_id, "dissenting-accept", 11),
            Err(StoreError::MissingPassingVerdict(head)) if head == candidate
        ));
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one end-to-end regression keeps the failed-SHA immutability and fresh-SHA recovery contract auditable"
    )]
    fn failed_verdict_is_immutable_and_recovery_requires_a_new_sha() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let mut brief = brief();
        brief.risk_class = RiskClass::Low;
        let failed_sha = sha('b');
        let replacement_sha = sha('c');
        store.create_job(&brief, 1).expect("job created");
        let coder = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(
                &coder,
                &brief.unit_id,
                &failed_sha,
                "failed-sha-candidate",
                7,
            )
            .expect("failed SHA recorded");
        let verifier = register_verifier(&mut store, &brief, "fail-closed", 8);
        store
            .transition(
                &verifier,
                &brief.unit_id,
                JobState::IndependentlyVerifying,
                "failed-sha-verification",
                8,
            )
            .expect("verification starts");
        store
            .record_verdict(
                &brief.unit_id,
                &VerificationVerdict {
                    verifier: &verifier,
                    head_sha: &failed_sha,
                    passed: false,
                    evidence: &json!({"result": "failed"}),
                    idempotency_key: "failed-sha-verdict",
                },
                9,
            )
            .expect("failure is durable");

        assert!(
            store
                .connection
                .execute(
                    "INSERT INTO verification_verdicts (unit_id, verifier_profile, head_sha, passed, evidence_json, created_at) VALUES (?1, ?2, ?3, 1, '{}', 10)",
                    rusqlite::params![
                        brief.unit_id.as_str(),
                        verifier.as_str(),
                        failed_sha.as_str()
                    ],
                )
                .is_err(),
            "one verifier cannot replace a failure with a second verdict for the same SHA"
        );
        assert!(
            store
                .connection
                .execute(
                    "UPDATE verification_verdicts SET passed = 1 WHERE unit_id = ?1 AND head_sha = ?2",
                    rusqlite::params![brief.unit_id.as_str(), failed_sha.as_str()],
                )
                .is_err(),
            "persisted verdicts cannot be revised"
        );
        assert!(
            store
                .connection
                .execute(
                    "DELETE FROM verification_verdicts WHERE unit_id = ?1 AND head_sha = ?2",
                    rusqlite::params![brief.unit_id.as_str(), failed_sha.as_str()],
                )
                .is_err(),
            "persisted verdicts cannot be erased"
        );
        let integrator = ensure_integrator(&mut store, &brief, 10);
        assert!(matches!(
            store.accept_verdict(&integrator, &brief.unit_id, "failed-sha-accept", 10),
            Err(StoreError::MissingPassingVerdict(head)) if head == failed_sha
        ));

        let coordinator = ensure_coordinator(&mut store, &brief, 11);
        store
            .transition(
                &coordinator,
                &brief.unit_id,
                JobState::FixRequired,
                "failed-sha-fix-required",
                11,
            )
            .expect("failed SHA enters repair");
        store
            .transition(
                &coordinator,
                &brief.unit_id,
                JobState::Leased,
                "replacement-leased",
                12,
            )
            .expect("replacement work leased");
        for (state, key, now) in [
            (JobState::Grounding, "replacement-grounding", 13),
            (JobState::Implementing, "replacement-implementing", 14),
            (JobState::SelfVerifying, "replacement-self-verifying", 15),
        ] {
            store
                .transition(&coder, &brief.unit_id, state, key, now)
                .expect("replacement advances");
        }
        store
            .record_candidate(
                &coder,
                &brief.unit_id,
                &replacement_sha,
                "replacement-candidate",
                16,
            )
            .expect("new SHA recorded");
        store
            .transition(
                &verifier,
                &brief.unit_id,
                JobState::IndependentlyVerifying,
                "replacement-verification",
                17,
            )
            .expect("new SHA verification starts");
        store
            .record_verdict(
                &brief.unit_id,
                &VerificationVerdict {
                    verifier: &verifier,
                    head_sha: &replacement_sha,
                    passed: true,
                    evidence: &json!({"result": "passed"}),
                    idempotency_key: "replacement-verdict",
                },
                18,
            )
            .expect("same verifier may assess a new SHA");
        assert_eq!(
            store
                .accept_verdict(&integrator, &brief.unit_id, "replacement-accept", 19)
                .expect("fresh SHA can recover"),
            JobState::Verified
        );
    }

    #[test]
    fn unattributed_legacy_verdict_cannot_authorize_a_sha() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let candidate = sha('b');
        store.create_job(&brief, 1).expect("job created");
        let coder = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(&coder, &brief.unit_id, &candidate, "legacy-candidate", 7)
            .expect("candidate recorded");
        store
            .connection
            .execute(
                "INSERT INTO verification_verdicts (unit_id, head_sha, passed, evidence_json, created_at) VALUES (?1, ?2, 1, '{}', 8)",
                rusqlite::params![brief.unit_id.as_str(), candidate.as_str()],
            )
            .expect("legacy verdict inserted");
        store
            .connection
            .execute(
                "UPDATE units SET state = 'reviewing' WHERE unit_id = ?1",
                [brief.unit_id.as_str()],
            )
            .expect("model migrated reviewing state");
        let integrator = ensure_integrator(&mut store, &brief, 9);
        assert!(matches!(
            store.accept_verdict(&integrator, &brief.unit_id, "legacy-accept", 9),
            Err(StoreError::MissingPassingVerdict(head)) if head == candidate
        ));
    }

    #[test]
    fn low_risk_and_reverified_integration_paths_are_explicit() {
        let candidate = sha('b');
        let mut low_risk = brief();
        low_risk.risk_class = RiskClass::Low;
        let mut store = ControlStore::open_in_memory().expect("store opens");
        prepare_reviewing_candidate(&mut store, &low_risk, &candidate);
        let integrator = ensure_integrator(&mut store, &low_risk, 10);
        assert_eq!(
            store
                .accept_verdict(&integrator, &low_risk.unit_id, "low-risk-accepted", 10)
                .expect("low risk needs no reviewer quorum"),
            JobState::Verified
        );
        assert_eq!(
            store
                .accept_verdict(&integrator, &low_risk.unit_id, "low-risk-accepted", 99)
                .expect("accepted verdict replays after state advance"),
            JobState::Verified
        );

        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        prepare_reviewing_candidate(&mut store, &brief, &candidate);
        record_two_reviews(&mut store, &brief, &candidate, 10);
        let integrator = ensure_integrator(&mut store, &brief, 10);
        store
            .connection
            .execute(
                "UPDATE units SET integration_sha = ?1 WHERE unit_id = ?2",
                rusqlite::params![candidate.as_str(), brief.unit_id.as_str()],
            )
            .expect("model a reverified integration candidate");
        assert_eq!(
            store
                .accept_verdict(&integrator, &brief.unit_id, "integration-reverified", 16)
                .expect("exact integration SHA is restored"),
            JobState::Integrated
        );
        store
            .recover_integration(
                &brief.unit_id,
                &integrator,
                JobState::Blocked,
                &json!({"reason": "integration environment rejected"}),
                "recover-integrated",
                17,
            )
            .expect("integrator can recover an integrated SHA");
    }

    #[test]
    fn review_and_path_guards_fail_before_mutation() {
        assert!(is_safe_relative_path(std::path::Path::new(
            "crates/assigned"
        )));
        for path in ["", "/absolute", "crates/../sibling"] {
            assert!(!is_safe_relative_path(std::path::Path::new(path)));
        }

        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let reviewer = ProfileId::new("early-reviewer").expect("reviewer id");
        let integrator = ProfileId::new("early-integrator").expect("integrator id");
        for (profile, role, home) in [
            (
                &reviewer,
                Role::VerifierReviewer,
                "/tmp/nswarm-early-reviewer",
            ),
            (
                &integrator,
                Role::Integrator,
                "/tmp/nswarm-early-integrator",
            ),
        ] {
            store
                .register_profile(
                    profile,
                    &brief.job_id,
                    &brief.unit_id,
                    role,
                    std::path::Path::new(home),
                    2,
                )
                .expect("profile registered");
        }
        assert!(matches!(
            store.record_review(
                &brief.unit_id,
                &reviewer,
                &sha('b'),
                ReviewAssessment::Noted,
                &json!({}),
                3,
            ),
            Err(StoreError::ReviewOutsideReviewState)
        ));
        assert!(matches!(
            store.dispose_review_finding(
                &brief.unit_id,
                &integrator,
                &FindingDisposition {
                    finding_id: 1,
                    disposition: ReviewAssessment::Noted,
                    rationale: &json!({}),
                    idempotency_key: "early-disposition",
                },
                3,
            ),
            Err(StoreError::ReviewOutsideReviewState)
        ));
    }

    fn assert_reverse_path_overlap_rejected(brief: &JobBrief) {
        let mut reverse_store = ControlStore::open_in_memory().expect("reverse store");
        reverse_store
            .create_job(brief, 1)
            .expect("reverse job created");
        let reverse_coordinator = ensure_coordinator(&mut reverse_store, brief, 2);
        let reverse_coder = ensure_profile(
            &mut reverse_store,
            brief,
            "reverse-lease-coder",
            Role::Coder,
            2,
        );
        reverse_store
            .acquire_lease(
                &reverse_coordinator,
                &reverse_coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Path,
                "crates/assigned/src",
                100,
                2,
            )
            .expect("child path leased");
        assert!(matches!(
            reverse_store.acquire_lease(
                &reverse_coordinator,
                &reverse_coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Path,
                "crates/assigned",
                100,
                3,
            ),
            Err(StoreError::LeaseConflict(_))
        ));
    }

    #[test]
    fn overlapping_and_topology_leases_are_rejected() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ensure_coordinator(&mut store, &brief, 2);
        let coder = ensure_profile(&mut store, &brief, "lease-coder-job-1", Role::Coder, 2);
        let integrator = ensure_integrator(&mut store, &brief, 2);
        store
            .acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Path,
                "crates/assigned",
                100,
                2,
            )
            .expect("first path lease");
        assert!(matches!(
            store.acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Path,
                "crates/assigned/src",
                100,
                3,
            ),
            Err(StoreError::LeaseConflict(_))
        ));
        assert_reverse_path_overlap_rejected(&brief);
        store
            .acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Path,
                "crates/other",
                100,
                3,
            )
            .expect("disjoint path lease");
        store
            .acquire_lease(
                &coordinator,
                &integrator,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Topology,
                "integration-stack",
                100,
                3,
            )
            .expect("first topology lease");
        assert!(matches!(
            store.acquire_lease(
                &coordinator,
                &integrator,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Topology,
                "other-integration-stack",
                100,
                4,
            ),
            Err(StoreError::LeaseConflict(_))
        ));

        let mut other = brief.clone();
        other.job_id = JobId::new("job-2").expect("other job");
        other.unit_id = UnitId::new("unit-2").expect("other unit");
        store.create_job(&other, 4).expect("other job created");
        let other_coordinator = ensure_coordinator(&mut store, &other, 5);
        let other_integrator = ensure_integrator(&mut store, &other, 5);
        store
            .acquire_lease(
                &other_coordinator,
                &other_integrator,
                &other.job_id,
                &other.unit_id,
                LeaseKind::Topology,
                "independent-integration-stack",
                100,
                5,
            )
            .expect("independent jobs may own topology concurrently");
    }

    #[test]
    fn invalid_path_leases_and_job_aliases_are_rejected() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ensure_coordinator(&mut store, &brief, 2);
        let coder = ensure_profile(&mut store, &brief, "invalid-lease-coder", Role::Coder, 2);
        assert!(matches!(
            store.acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Path,
                "crates/other/../assigned",
                100,
                3,
            ),
            Err(StoreError::InvalidLease)
        ));
        for (resource, expires_at) in [("", 100), ("crates/other", 3)] {
            assert!(matches!(
                store.acquire_lease(
                    &coordinator,
                    &coder,
                    &brief.job_id,
                    &brief.unit_id,
                    LeaseKind::Path,
                    resource,
                    expires_at,
                    3,
                ),
                Err(StoreError::InvalidLease)
            ));
        }
        assert!(matches!(
            store.acquire_lease(
                &coordinator,
                &coder,
                &JobId::new("job-2").expect("other job"),
                &brief.unit_id,
                LeaseKind::Path,
                "crates/other",
                100,
                3,
            ),
            Err(StoreError::JobUnitMismatch)
        ));
        assert!(matches!(
            store.acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                "different-profile",
                100,
                3,
            ),
            Err(StoreError::InvalidLease)
        ));
    }

    #[test]
    fn zombie_result_is_durably_quarantined() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ensure_coordinator(&mut store, &brief, 2);
        let coder = ensure_profile(&mut store, &brief, "zombie-coder", Role::Coder, 2);
        store
            .transition(&coordinator, &brief.unit_id, JobState::Leased, "leased", 2)
            .expect("leased");
        let lease = store
            .acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                coder.as_str(),
                10,
                2,
            )
            .expect("lease acquired");
        assert!(matches!(
            store.accept_worker_result(&coder, &brief.unit_id, lease, &sha('b'), 11),
            Err(StoreError::StaleLease(_))
        ));
        assert_eq!(
            store.state(&brief.unit_id).expect("state"),
            JobState::Quarantined
        );
        assert!(matches!(
            store.accept_worker_result(&coder, &brief.unit_id, lease, &sha('c'), 11),
            Err(StoreError::StaleLease(id)) if id == lease
        ));
    }

    #[test]
    fn cross_job_dependencies_are_rejected_and_rolled_back() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let prerequisite = brief();
        store
            .create_job(&prerequisite, 1)
            .expect("prerequisite created");
        let mut dependent = brief();
        dependent.job_id = JobId::new("job-2").expect("dependent job");
        dependent.unit_id = UnitId::new("unit-2").expect("dependent unit");
        dependent.dependencies = vec![prerequisite.unit_id.clone()];
        assert!(matches!(
            store.create_job(&dependent, 2),
            Err(StoreError::UnknownDependency(unit)) if unit == prerequisite.unit_id.as_str()
        ));
        let partial_rows: i64 = store
            .connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM jobs WHERE job_id = 'job-2') + (SELECT COUNT(*) FROM units WHERE unit_id = 'unit-2') + (SELECT COUNT(*) FROM unit_briefs WHERE unit_id = 'unit-2') + (SELECT COUNT(*) FROM dependencies WHERE unit_id = 'unit-2') + (SELECT COUNT(*) FROM events WHERE job_id = 'job-2')",
                [],
                |row| row.get(0),
            )
            .expect("cross-job rollback query");
        assert_eq!(partial_rows, 0);

        dependent.dependencies.clear();
        store
            .create_job(&dependent, 3)
            .expect("independent second job created");
        assert!(
            store
                .connection
                .execute(
                    "INSERT INTO dependencies (unit_id, depends_on_unit_id) VALUES (?1, ?2)",
                    rusqlite::params![dependent.unit_id.as_str(), prerequisite.unit_id.as_str()],
                )
                .is_err()
        );
    }

    #[test]
    fn one_job_can_own_multiple_unit_briefs_and_dependencies() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let first = brief();
        store.create_job(&first, 1).expect("first unit created");

        let mut second = first.clone();
        second.unit_id = UnitId::new("unit-2").expect("second unit");
        second.dependencies = vec![first.unit_id.clone()];
        second.report_schema = json!({
            "type": "object",
            "required": ["status"],
            "properties": {"status": {"type": "string"}},
            "additionalProperties": false
        });
        store.create_job(&second, 2).expect("second unit created");
        let second_coordinator = ensure_coordinator(&mut store, &second, 3);
        let second_coder = ensure_profile(&mut store, &second, "second-unit-coder", Role::Coder, 3);
        let first_coder = ensure_profile(&mut store, &first, "first-unit-reporter", Role::Coder, 3);
        ensure_profile_lease(&mut store, &first, &first_coder, 3);

        let (jobs, units, briefs): (i64, i64, i64) = store
            .connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM jobs), (SELECT COUNT(*) FROM units), (SELECT COUNT(*) FROM unit_briefs)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("inventory query");
        assert_eq!((jobs, units, briefs), (1, 2, 2));
        assert!(matches!(
            store.acquire_lease(
                &second_coordinator,
                &second_coder,
                &second.job_id,
                &second.unit_id,
                LeaseKind::Path,
                "crates/second",
                100,
                3,
            ),
            Err(StoreError::DependenciesUnsatisfied(1))
        ));
        store
            .connection
            .execute(
                "UPDATE units SET state = 'merged' WHERE unit_id = ?1",
                [first.unit_id.as_str()],
            )
            .expect("prerequisite merged");
        store
            .acquire_lease(
                &second_coordinator,
                &second_coder,
                &second.job_id,
                &second.unit_id,
                LeaseKind::Path,
                "crates/second",
                100,
                4,
            )
            .expect("dependency now satisfied");
        ensure_profile_lease(&mut store, &second, &second_coder, 4);
        store
            .record_report(
                &second_coder,
                &second.unit_id,
                &json!({"status": "complete"}),
                "second-unit-report",
                5,
            )
            .expect("second unit uses its own report schema");
        assert!(matches!(
            store.record_report(
                &first_coder,
                &first.unit_id,
                &json!({"status": "complete"}),
                "wrong-first-unit-schema",
                6,
            ),
            Err(StoreError::ReportSchemaViolation)
        ));
    }

    #[test]
    fn unknown_dependencies_roll_back_the_whole_unit_creation() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let mut dependent = brief();
        dependent.dependencies = vec![UnitId::new("missing-unit").expect("dependency id")];
        assert!(matches!(
            store.create_job(&dependent, 1),
            Err(StoreError::UnknownDependency(unit)) if unit == "missing-unit"
        ));
        let counts: (i64, i64, i64, i64, i64) = store
            .connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM jobs), (SELECT COUNT(*) FROM units), (SELECT COUNT(*) FROM unit_briefs), (SELECT COUNT(*) FROM dependencies), (SELECT COUNT(*) FROM events)",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("inventory query");
        assert_eq!(counts, (0, 0, 0, 0, 0));
    }

    #[test]
    fn units_cannot_redefine_job_scope_or_credential_methods() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let first = brief();
        store.create_job(&first, 1).expect("first unit created");

        let mut changed_scope = first.clone();
        changed_scope.unit_id = UnitId::new("unit-scope").expect("unit id");
        changed_scope.repository = "https://example.invalid/other.git".to_owned();
        assert!(matches!(
            store.create_job(&changed_scope, 2),
            Err(StoreError::JobScopeMismatch(job)) if job == first.job_id.as_str()
        ));

        let mut changed_policy = first.clone();
        changed_policy.unit_id = UnitId::new("unit-policy").expect("unit id");
        changed_policy.standing_policy_version = "v2".to_owned();
        assert!(matches!(
            store.create_job(&changed_policy, 3),
            Err(StoreError::JobScopeMismatch(job)) if job == first.job_id.as_str()
        ));

        let mut changed_grant = first.clone();
        changed_grant.unit_id = UnitId::new("unit-grant").expect("unit id");
        changed_grant.credential_grants[0].methods =
            vec!["git:push:refs/heads/nswarm/job-1/unit-grant".to_owned()];
        assert!(matches!(
            store.create_job(&changed_grant, 4),
            Err(StoreError::CredentialGrantConflict(grant)) if grant == "github-job-push"
        ));
        let rolled_back: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM units WHERE unit_id IN ('unit-scope', 'unit-policy', 'unit-grant')",
                [],
                |row| row.get(0),
            )
            .expect("rollback query");
        assert_eq!(rolled_back, 0);

        assert!(
            store
                .connection
                .execute(
                    "UPDATE jobs SET standing_policy_version = 'v2' WHERE job_id = ?1",
                    [first.job_id.as_str()],
                )
                .is_err()
        );
        assert!(
            store
                .connection
                .execute(
                    "UPDATE unit_briefs SET brief_json = '{}' WHERE unit_id = ?1",
                    [first.unit_id.as_str()],
                )
                .is_err()
        );
    }

    #[test]
    fn evidence_events_are_idempotent() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let first = store
            .append_event(
                &brief.job_id,
                "claim-1",
                "claim",
                &json!({"kind": "direct"}),
                2,
            )
            .expect("first append");
        let second = store
            .append_event(
                &brief.job_id,
                "claim-1",
                "claim",
                &json!({"kind": "direct"}),
                2,
            )
            .expect("idempotent append");
        assert_eq!(first, second);
        assert!(matches!(
            store.append_event(
                &brief.job_id,
                "claim-1",
                "claim",
                &json!({"kind": "different"}),
                3,
            ),
            Err(StoreError::IdempotencyConflict(key)) if key == "claim-1"
        ));
        assert!(matches!(
            store.append_event(
                &brief.job_id,
                "claim-1",
                "different-type",
                &json!({"kind": "direct"}),
                3,
            ),
            Err(StoreError::IdempotencyConflict(key)) if key == "claim-1"
        ));

        let mut other = brief.clone();
        other.job_id = JobId::new("job-2").expect("other job");
        other.unit_id = UnitId::new("unit-2").expect("other unit");
        store.create_job(&other, 3).expect("other job created");
        assert!(matches!(
            store.append_event(
                &other.job_id,
                "claim-1",
                "claim",
                &json!({"kind": "direct"}),
                3,
            ),
            Err(StoreError::IdempotencyConflict(key)) if key == "claim-1"
        ));

        for (key, event_type) in [("", "claim"), ("claim-2", " ")] {
            assert!(matches!(
                store.append_event(&brief.job_id, key, event_type, &json!({}), 4),
                Err(StoreError::InvalidEvent)
            ));
        }
    }

    #[test]
    fn compound_store_guards_reject_each_invalid_operand() {
        assert_eq!(ReviewAssessment::Blocking.as_str(), "blocking");
        assert_eq!(ReviewAssessment::Consider.as_str(), "consider");
        assert_eq!(ReviewAssessment::Noted.as_str(), "noted");
        assert_eq!(ReviewAssessment::Dismissed.as_str(), "dismissed");

        let brief = brief();
        let mut store = ControlStore::open_in_memory().expect("store opens");
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ensure_coordinator(&mut store, &brief, 2);
        let first = ensure_profile(&mut store, &brief, "profile-one", Role::Coder, 2);
        let second = ensure_profile(&mut store, &brief, "profile-two", Role::Coder, 2);

        store
            .acquire_lease(
                &coordinator,
                &first,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                first.as_str(),
                100,
                2,
            )
            .expect("first profile lease");
        assert!(matches!(
            store.acquire_lease(
                &coordinator,
                &first,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                first.as_str(),
                100,
                3,
            ),
            Err(StoreError::LeaseConflict(resource)) if resource == first.as_str()
        ));
        store
            .acquire_lease(
                &coordinator,
                &second,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                second.as_str(),
                100,
                3,
            )
            .expect("different profile lease");
    }

    #[test]
    fn branch_and_artifact_guards_reject_each_invalid_operand() {
        let brief = brief();
        let mut store = ControlStore::open_in_memory().expect("store opens");
        store.create_job(&brief, 1).expect("job created");
        let actor = ProfileId::new("invalid-branch-actor").expect("profile id");
        for worktree in ["relative/worktree", "/tmp/../escape"] {
            assert!(matches!(
                store.register_branch(
                    &actor,
                    &brief.unit_id,
                    "nswarm/job-1/unit-1",
                    std::path::Path::new(worktree),
                    &brief.base_sha,
                    4,
                ),
                Err(StoreError::InvalidWorktree(_))
            ));
        }
        for (name, base) in [
            ("nswarm/wrong/unit-1", brief.base_sha.clone()),
            ("nswarm/job-1/unit-1", sha('b')),
        ] {
            let coder = ensure_profile(&mut store, &brief, "branch-guard-coder", Role::Coder, 3);
            ensure_profile_lease(&mut store, &brief, &coder, 3);
            assert!(matches!(
                store.register_branch(
                    &coder,
                    &brief.unit_id,
                    name,
                    std::path::Path::new("/tmp/nswarm-worktrees/unit-1"),
                    &base,
                    4,
                ),
                Err(StoreError::InvalidBranchAssignment)
            ));
        }
        let coder = ensure_profile(&mut store, &brief, "branch-guard-coder", Role::Coder, 3);
        store
            .register_branch(
                &coder,
                &brief.unit_id,
                "nswarm/job-1/unit-1",
                std::path::Path::new("/tmp/nswarm-worktrees/unit-1"),
                &brief.base_sha,
                4,
            )
            .expect("valid branch registration");
        for artifact_path in ["", "/absolute/report.json", "artifacts/../secret"] {
            assert!(matches!(
                store.record_artifact(
                    &coder,
                    &brief.unit_id,
                    ArtifactKind::Log,
                    std::path::Path::new(artifact_path),
                    &brief.base_sha,
                    &sha('d'),
                    5,
                ),
                Err(StoreError::InvalidArtifact)
            ));
        }

        let coordinator = ensure_coordinator(&mut store, &brief, 6);
        store
            .transition(
                &coordinator,
                &brief.unit_id,
                JobState::Leased,
                "state-leased",
                6,
            )
            .expect("unit leased");
        for state in [JobState::Grounding, JobState::Implementing] {
            store
                .transition(
                    &coder,
                    &brief.unit_id,
                    state,
                    &format!("state-{}", state.as_str()),
                    6,
                )
                .expect("coding state advances");
        }
        assert!(matches!(
            store.update_branch_head(&coder, &brief.unit_id, &brief.base_sha, &sha('b'), "", 7),
            Err(StoreError::InvalidEvent)
        ));
    }

    #[test]
    fn secret_shape_detection_covers_each_supported_token_family() {
        let secrets = [
            format!("-----BEGIN {}-----", "PRIVATE KEY"),
            "Bearer abcdefghijklmnopqrst".to_owned(),
            "ghp_".to_owned() + &"a".repeat(32),
            "gho_".to_owned() + &"b".repeat(32),
            "ghu_".to_owned() + &"c".repeat(32),
            "ghs_".to_owned() + &"d".repeat(32),
            "ghr_".to_owned() + &"e".repeat(32),
            "github_pat_".to_owned() + &"f".repeat(32),
            "sk-".to_owned() + &"g".repeat(21),
            "sk_".to_owned() + &"h".repeat(21),
            "AKIA".to_owned() + &"I".repeat(16),
        ];
        for secret in secrets {
            assert!(contains_secret_shape(&secret), "missed {secret}");
        }
        for ordinary in [
            "Bearer abcdefghijklmnopqrs",
            "ghp_short",
            "sk-short",
            "AKIAABCDEFGHIJKLMNO",
            "ordinary evidence",
        ] {
            assert!(!contains_secret_shape(ordinary), "redacted {ordinary}");
        }
    }

    #[test]
    fn eval_redaction_corpus_uses_production_filter() {
        let case: serde_json::Value =
            serde_json::from_str(include_str!("../../../eval/corpus/redaction.json"))
                .expect("redaction eval corpus parses");
        let mut evidence = case["input"]["evidence"].clone();
        evidence["nested"][1]["note"] = serde_json::Value::String(
            case["input"]["token_fragments"]
                .as_array()
                .expect("token fragments are an array")
                .iter()
                .map(|fragment| fragment.as_str().expect("token fragment is text"))
                .collect(),
        );
        assert_eq!(redact_evidence(&evidence), case["expected"]["evidence"]);
    }

    #[test]
    fn review_authorization_requires_both_job_and_role() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let candidate = sha('b');
        prepare_reviewing_candidate(&mut store, &brief, &candidate);

        let same_job_coder = ProfileId::new("same-job-coder").expect("profile id");
        store
            .register_profile(
                &same_job_coder,
                &brief.job_id,
                &brief.unit_id,
                Role::Coder,
                std::path::Path::new("/tmp/nswarm-same-job-coder"),
                10,
            )
            .expect("same-job coder registered");

        let mut other = brief.clone();
        other.job_id = JobId::new("job-2").expect("other job");
        other.unit_id = UnitId::new("unit-2").expect("other unit");
        store.create_job(&other, 10).expect("other job created");
        let other_job_reviewer = ProfileId::new("other-job-reviewer").expect("profile id");
        store
            .register_profile(
                &other_job_reviewer,
                &other.job_id,
                &other.unit_id,
                Role::VerifierReviewer,
                std::path::Path::new("/tmp/nswarm-other-job-reviewer"),
                11,
            )
            .expect("other-job reviewer registered");

        for reviewer in [&same_job_coder, &other_job_reviewer] {
            assert!(matches!(
                store.record_review(
                    &brief.unit_id,
                    reviewer,
                    &candidate,
                    ReviewAssessment::Noted,
                    &json!({"summary": "must not be accepted"}),
                    12,
                ),
                Err(StoreError::UnauthorizedReviewer)
            ));
        }
    }

    #[test]
    fn worker_reports_are_schema_checked_and_redacted_before_storage() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let reporter = ensure_profile(&mut store, &brief, "reporter", Role::Coder, 2);
        ensure_profile_lease(&mut store, &brief, &reporter, 2);
        assert!(matches!(
            store.record_report(
                &reporter,
                &brief.unit_id,
                &json!({"head_sha": sha('b').as_str()}),
                "incomplete-report",
                2,
            ),
            Err(StoreError::ReportSchemaViolation)
        ));
        assert!(matches!(
            store.record_report(
                &reporter,
                &brief.unit_id,
                &json!({"head_sha": sha('b').as_str(), "evidence": "not-an-object"}),
                "wrong-type-report",
                2,
            ),
            Err(StoreError::ReportSchemaViolation)
        ));
        assert!(matches!(
            store.record_report(
                &reporter,
                &brief.unit_id,
                &json!({
                    "head_sha": sha('b').as_str(),
                    "evidence": {"checks": [1]},
                    "undeclared": true
                }),
                "recursive-wrong-type-report",
                2,
            ),
            Err(StoreError::ReportSchemaViolation)
        ));

        let provider_token = "sk-".to_owned() + &"x".repeat(24);
        let source_token = "ghp_".to_owned() + &"y".repeat(30);
        let modern_token = "github_pat_".to_owned() + &"z".repeat(32);
        let bearer_token = "t0k3n".to_owned() + &"q".repeat(27);
        let private_marker = format!("-----BEGIN {} {}-----", "PRIVATE", "KEY");
        store
            .record_report(
                &reporter,
                &brief.unit_id,
                &json!({
                    "head_sha": sha('b').as_str(),
                    "evidence": {
                        "checks": ["focused test passed"],
                        "details": {
                            "OPENROUTER_API_KEY": provider_token,
                            "apiKey": "synthetic-camel-case-key",
                            "nested": [
                                {"authorization": source_token},
                                {"note": modern_token},
                                {"privateMaterial": private_marker},
                                {"header": format!("Bearer {bearer_token}")}
                            ]
                        }
                    }
                }),
                "complete-report",
                3,
            )
            .expect("valid report recorded");

        let stored: String = store
            .connection
            .query_row(
                "SELECT payload_json FROM events WHERE idempotency_key = ?1",
                ["complete-report"],
                |row| row.get(0),
            )
            .expect("stored report");
        assert!(!stored.contains(&provider_token));
        assert!(!stored.contains(&source_token));
        assert!(!stored.contains(&modern_token));
        assert!(!stored.contains(&bearer_token));
        assert!(!stored.contains("BEGIN PRIVATE KEY"));
        assert!(!stored.contains("synthetic-camel-case-key"));
        assert_eq!(stored.matches("[REDACTED]").count(), 6);
        assert!(stored.contains("focused test passed"));
    }

    #[test]
    fn verdict_and_review_evidence_are_redacted_before_storage() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        let candidate = sha('b');
        store.create_job(&brief, 1).expect("job created");
        let coder = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(&coder, &unit, &candidate, "candidate-redaction", 7)
            .expect("candidate recorded");
        let reviewer = register_verifier(&mut store, &brief, "redaction", 8);
        store
            .transition(
                &reviewer,
                &unit,
                JobState::IndependentlyVerifying,
                "verify-redaction",
                8,
            )
            .expect("verification starts");
        let token = "sk-".to_owned() + &"z".repeat(24);
        store
            .record_verdict(
                &unit,
                &VerificationVerdict {
                    verifier: &reviewer,
                    head_sha: &candidate,
                    passed: true,
                    evidence: &json!({"provider_token": token, "result": "pass"}),
                    idempotency_key: "verdict-redaction",
                },
                9,
            )
            .expect("verdict recorded");
        store
            .record_review(
                &unit,
                &reviewer,
                &candidate,
                ReviewAssessment::Noted,
                &json!({"authorization": token, "summary": "reviewed"}),
                11,
            )
            .expect("review recorded");

        for (table, column) in [
            ("verification_verdicts", "evidence_json"),
            ("review_findings", "finding_json"),
        ] {
            let query = format!("SELECT {column} FROM {table} ORDER BY rowid DESC LIMIT 1");
            let stored: String = store
                .connection
                .query_row(&query, [], |row| row.get(0))
                .expect("stored evidence");
            assert!(!stored.contains(&token));
            assert!(stored.contains("[REDACTED]"));
        }
        let verdict_actor: String = store
            .connection
            .query_row(
                "SELECT verifier_profile FROM verification_verdicts ORDER BY verdict_id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("verdict actor stored");
        assert_eq!(verdict_actor, reviewer.as_str());
    }

    fn assert_integration_completion_actor_guards(
        store: &mut ControlStore,
        brief: &JobBrief,
        candidate: &Sha,
        coder: &ProfileId,
    ) {
        let topology_free_integrator = ensure_profile(
            store,
            brief,
            "topology-free-integrator",
            Role::Integrator,
            16,
        );
        ensure_profile_lease(store, brief, &topology_free_integrator, 16);
        assert!(matches!(
            store.complete_integration(
                &topology_free_integrator,
                &brief.unit_id,
                candidate,
                "topology-free-integration",
                17,
            ),
            Err(StoreError::MissingActorLease {
                actor,
                kind: LeaseKind::Topology
            }) if actor == topology_free_integrator.as_str()
        ));
        assert!(matches!(
            store.complete_integration(
                coder,
                &brief.unit_id,
                candidate,
                "coder-integration",
                17,
            ),
            Err(StoreError::UnauthorizedActor(actor)) if actor == coder.as_str()
        ));
    }

    fn assert_recovery_cannot_synthesize_merge_authorization(
        store: &mut ControlStore,
        unit: &UnitId,
        integrator: &ProfileId,
    ) {
        assert!(matches!(
            store.recover_integration(
                unit,
                integrator,
                JobState::MergeAuthorized,
                &json!({"reason": "authorization cannot be synthesized by recovery"}),
                "invalid-recovery-edge",
                18,
            ),
            Err(StoreError::InvalidRecovery {
                current: JobState::Integrated,
                next: JobState::MergeAuthorized
            })
        ));
    }

    fn assert_merge_authorization_is_actor_bound(
        store: &mut ControlStore,
        brief: &JobBrief,
        candidate: &Sha,
    ) {
        let wrong_role = ensure_profile(store, brief, "wrong-role-shipper", Role::Coder, 21);
        assert!(matches!(
            store.record_merged(
                &brief.unit_id,
                candidate,
                &wrong_role,
                "wrong-role-shipper",
                21,
            ),
            Err(StoreError::UnauthorizedShipper)
        ));
        let different_shipper =
            ensure_profile(store, brief, "different-shipper", Role::Shipper, 21);
        assert!(matches!(
            store.record_merged(
                &brief.unit_id,
                candidate,
                &different_shipper,
                "wrong-shipper",
                21,
            ),
            Err(StoreError::UnauthorizedShipper)
        ));
    }

    #[test]
    fn exact_sha_can_complete_the_full_authorized_lifecycle() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        let candidate = sha('b');
        store.create_job(&brief, 1).expect("job created");
        let coder = advance_to_self_verifying(&mut store, &brief);
        store
            .record_candidate(&coder, &unit, &candidate, "candidate-full", 7)
            .expect("candidate recorded");
        let verifier = register_verifier(&mut store, &brief, "full", 8);
        store
            .transition(
                &verifier,
                &unit,
                JobState::IndependentlyVerifying,
                "verify-full",
                8,
            )
            .expect("verification starts");
        store
            .record_verdict(
                &unit,
                &VerificationVerdict {
                    verifier: &verifier,
                    head_sha: &candidate,
                    passed: true,
                    evidence: &json!({"commands": ["cargo test"], "artifacts": []}),
                    idempotency_key: "verdict-full",
                },
                9,
            )
            .expect("verdict recorded");
        record_two_reviews(&mut store, &brief, &candidate, 10);
        let integrator = ensure_integrator(&mut store, &brief, 10);
        assert_eq!(
            store
                .accept_verdict(&integrator, &unit, "accept-full", 15)
                .expect("accepted"),
            JobState::Verified
        );
        ensure_topology_lease(&mut store, &brief, &integrator, 16);
        store
            .transition(
                &integrator,
                &unit,
                JobState::Integrating,
                "integrate-full",
                16,
            )
            .expect("integration starts");
        assert_integration_completion_actor_guards(&mut store, &brief, &candidate, &coder);
        assert_eq!(
            store
                .complete_integration(&integrator, &unit, &candidate, "integrated-full", 17)
                .expect("unchanged integration remains verified"),
            JobState::Integrated
        );
        assert_recovery_cannot_synthesize_merge_authorization(&mut store, &unit, &integrator);
        let unregistered = ProfileId::new("unregistered-shipper").expect("shipper id");
        assert!(matches!(
            store.authorize_merge(&unit, &candidate, &unregistered, "untrusted-auth", 18),
            Err(StoreError::UnauthorizedShipper)
        ));
        let shipper = register_shipper(&mut store, &brief, 18);
        assert!(matches!(
            store.record_merged(&unit, &candidate, &shipper, "premature-merge", 19),
            Err(StoreError::InvalidTransition { .. })
        ));
        assert!(matches!(
            store.authorize_merge(&unit, &sha('c'), &shipper, "wrong-auth", 19),
            Err(StoreError::UnauthorizedSha { .. })
        ));
        store
            .authorize_merge(&unit, &candidate, &shipper, "authorize-full", 20)
            .expect("exact SHA authorized");
        assert_merge_authorization_is_actor_bound(&mut store, &brief, &candidate);
        assert!(matches!(
            store.record_merged(&unit, &sha('c'), &shipper, "wrong-merge", 21),
            Err(StoreError::UnauthorizedSha { .. })
        ));
        store
            .record_merged(&unit, &candidate, &shipper, "merged-full", 22)
            .expect("exact SHA merged");
        store
            .record_merged(&unit, &candidate, &shipper, "merged-full", 99)
            .expect("merge completion replays after state advance");
        assert_eq!(store.state(&unit).expect("state"), JobState::Merged);
    }

    fn authorize_initial_candidate(
        store: &mut ControlStore,
        brief: &JobBrief,
        candidate: &Sha,
    ) -> ProfileId {
        prepare_reviewing_candidate(store, brief, candidate);
        let integrator = ensure_integrator(store, brief, 10);
        assert_eq!(
            store
                .accept_verdict(&integrator, &brief.unit_id, "accept-first", 10)
                .expect("first SHA accepted"),
            JobState::Verified
        );
        ensure_topology_lease(store, brief, &integrator, 11);
        store
            .transition(
                &integrator,
                &brief.unit_id,
                JobState::Integrating,
                "integrate-first",
                11,
            )
            .expect("first integration starts");
        assert_eq!(
            store
                .complete_integration(
                    &integrator,
                    &brief.unit_id,
                    candidate,
                    "integrated-first",
                    12,
                )
                .expect("first integration completes"),
            JobState::Integrated
        );
        let shipper = register_shipper(store, brief, 13);
        store
            .authorize_merge(&brief.unit_id, candidate, &shipper, "authorize-first", 14)
            .expect("first SHA authorized");
        shipper
    }

    fn recover_authorized_candidate(
        store: &mut ControlStore,
        brief: &JobBrief,
        shipper: &ProfileId,
    ) {
        let unit = &brief.unit_id;
        let integrator = ProfileId::new("recovery-integrator").expect("integrator id");
        store
            .register_profile(
                &integrator,
                &brief.job_id,
                unit,
                Role::Integrator,
                std::path::Path::new("/tmp/nswarm-recovery-integrator"),
                15,
            )
            .expect("integrator registered");
        assert!(matches!(
            store.recover_integration(
                unit,
                &integrator,
                JobState::FixRequired,
                &json!({"reason": "protected merge rejected"}),
                "wrong-recovery-role",
                16,
            ),
            Err(StoreError::UnauthorizedRecovery)
        ));
        store
            .recover_integration(
                unit,
                shipper,
                JobState::FixRequired,
                &json!({"reason": "protected merge rejected"}),
                "recover-first",
                16,
            )
            .expect("shipper recovers rejected merge");
        store
            .recover_integration(
                unit,
                shipper,
                JobState::FixRequired,
                &json!({"reason": "protected merge rejected"}),
                "recover-first",
                99,
            )
            .expect("identical recovery replay succeeds after state advance");
        assert!(matches!(
            store.recover_integration(
                unit,
                shipper,
                JobState::Blocked,
                &json!({"reason": "different recovery"}),
                "recover-first",
                99,
            ),
            Err(StoreError::IdempotencyConflict(key)) if key == "recover-first"
        ));
    }

    fn authorize_replacement_candidate(
        store: &mut ControlStore,
        brief: &JobBrief,
        candidate: &Sha,
        shipper: &ProfileId,
    ) {
        let unit = &brief.unit_id;
        let coordinator = ensure_coordinator(store, brief, 17);
        let coder = ensure_profile(
            store,
            brief,
            &format!("coder-{}-{}", brief.job_id, brief.unit_id),
            Role::Coder,
            17,
        );
        store
            .transition(&coordinator, unit, JobState::Leased, "second-leased", 17)
            .expect("replacement unit leased");
        for (state, key, timestamp) in [
            (JobState::Grounding, "second-grounding", 18),
            (JobState::Implementing, "second-implementing", 19),
            (JobState::SelfVerifying, "second-self-verifying", 20),
        ] {
            store
                .transition(&coder, unit, state, key, timestamp)
                .expect("second candidate advances");
        }
        store
            .record_candidate(&coder, unit, candidate, "candidate-second", 21)
            .expect("second candidate recorded");
        let verifier = register_verifier(store, brief, "second-sha", 22);
        store
            .transition(
                &verifier,
                unit,
                JobState::IndependentlyVerifying,
                "verify-second",
                22,
            )
            .expect("second verification starts");
        store
            .record_verdict(
                unit,
                &VerificationVerdict {
                    verifier: &verifier,
                    head_sha: candidate,
                    passed: true,
                    evidence: &json!({"commands": ["cargo test"]}),
                    idempotency_key: "verdict-second",
                },
                23,
            )
            .expect("second SHA freshly verified");
        let integrator = ensure_integrator(store, brief, 24);
        assert_eq!(
            store
                .accept_verdict(&integrator, unit, "accept-second", 24)
                .expect("second SHA accepted"),
            JobState::Verified
        );
        ensure_topology_lease(store, brief, &integrator, 25);
        store
            .transition(
                &integrator,
                unit,
                JobState::Integrating,
                "integrate-second",
                25,
            )
            .expect("second integration starts");
        assert_eq!(
            store
                .complete_integration(&integrator, unit, candidate, "integrated-second", 26)
                .expect("second integration completes"),
            JobState::Integrated
        );
        store
            .authorize_merge(unit, candidate, shipper, "authorize-second", 27)
            .expect("second SHA authorized");
        store
            .authorize_merge(unit, candidate, shipper, "authorize-second", 27)
            .expect("identical authorization replay succeeds after state advance");
    }

    #[test]
    fn merge_recovery_preserves_history_and_requires_fresh_sha_authorization() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let mut brief = brief();
        brief.risk_class = RiskClass::Low;
        let unit = brief.unit_id.clone();
        let first = sha('b');
        let second = sha('c');
        let shipper = authorize_initial_candidate(&mut store, &brief, &first);
        recover_authorized_candidate(&mut store, &brief, &shipper);
        authorize_replacement_candidate(&mut store, &brief, &second, &shipper);

        assert!(matches!(
            store.authorize_merge(&unit, &first, &shipper, "authorize-second", 28),
            Err(StoreError::IdempotencyConflict(key)) if key == "authorize-second"
        ));
        assert!(matches!(
            store.record_merged(&unit, &first, &shipper, "stale-first-merge", 28),
            Err(StoreError::UnauthorizedSha { expected, actual })
                if expected == second && actual == first
        ));
        assert!(
            store
                .connection
                .execute(
                    "INSERT INTO merge_authorizations (unit_id, head_sha, authorized_by, created_at) VALUES (?1, ?2, ?3, 28)",
                    rusqlite::params![unit.as_str(), first.as_str(), shipper.as_str()],
                )
                .is_err(),
            "storage allows only one active authorization per unit"
        );

        let (authorizations, active, events, commands): (i64, i64, i64, i64) = store
            .connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM merge_authorizations), (SELECT COUNT(*) FROM merge_authorizations WHERE invalidated_at IS NULL), (SELECT COUNT(*) FROM events WHERE idempotency_key = 'authorize-second'), (SELECT COUNT(*) FROM command_results WHERE idempotency_key = 'authorize-second')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("authorization history query");
        assert_eq!((authorizations, active, events, commands), (2, 1, 1, 1));
        store
            .record_merged(&unit, &second, &shipper, "merge-second", 29)
            .expect("only second SHA can merge");
    }

    #[test]
    fn review_gate_requires_live_integrator_disposition() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        let candidate = sha('b');
        prepare_reviewing_candidate(&mut store, &brief, &candidate);

        let (reviewers, findings) = record_unresolved_reviews(&mut store, &brief, &candidate);
        let integrator = ensure_integrator(&mut store, &brief, 14);
        assert!(matches!(
            store.accept_verdict(&integrator, &unit, "unresolved-accept", 15),
            Err(StoreError::ReviewGateUnsatisfied {
                reviewers: 2,
                unresolved: 2,
                blocking: 0
            })
        ));
        assert!(matches!(
            store.dispose_review_finding(
                &unit,
                &reviewers[0],
                &FindingDisposition {
                    finding_id: findings[0],
                    disposition: ReviewAssessment::Noted,
                    rationale: &json!({"reason": "attempted self-disposition"}),
                    idempotency_key: "self-disposition",
                },
                16,
            ),
            Err(StoreError::UnauthorizedIntegrator)
        ));

        for (index, finding) in findings.into_iter().enumerate() {
            let disposition = if index == 0 {
                ReviewAssessment::Blocking
            } else {
                ReviewAssessment::Noted
            };
            store
                .dispose_review_finding(
                    &unit,
                    &integrator,
                    &FindingDisposition {
                        finding_id: finding,
                        disposition,
                        rationale: &json!({"reason": "accepted with evidence"}),
                        idempotency_key: &format!("integrator-disposition-{index}"),
                    },
                    18 + i64::try_from(index).expect("small index"),
                )
                .expect("integrator disposition recorded");
            if index == 0 {
                assert!(matches!(
                    store.dispose_review_finding(
                        &unit,
                        &integrator,
                        &FindingDisposition {
                            finding_id: finding,
                            disposition: ReviewAssessment::Dismissed,
                            rationale: &json!({"reason": "second disposition"}),
                            idempotency_key: "duplicate-disposition",
                        },
                        20,
                    ),
                    Err(StoreError::UnknownOpenFinding(id)) if id == finding
                ));
            }
        }
        assert!(matches!(
            store.accept_verdict(&integrator, &unit, "blocking-accept", 22),
            Err(StoreError::ReviewGateUnsatisfied {
                reviewers: 2,
                unresolved: 0,
                blocking: 1
            })
        ));
    }

    #[test]
    fn live_worker_result_and_expired_lease_replacement_are_explicit() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ensure_coordinator(&mut store, &brief, 2);
        let coder = ensure_profile(&mut store, &brief, "coder-job-1-unit-1", Role::Coder, 2);
        store
            .transition(
                &coordinator,
                &brief.unit_id,
                JobState::Leased,
                "leased-live",
                2,
            )
            .expect("leased");
        let first = store
            .acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                "coder-job-1-unit-1",
                10,
                2,
            )
            .expect("first lease");
        store
            .accept_worker_result(&coder, &brief.unit_id, first, &sha('b'), 5)
            .expect("live result accepted");
        let second = store
            .acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                "coder-job-1-unit-1",
                20,
                11,
            )
            .expect("expired lease is closed before replacement");
        assert_ne!(first, second);
        assert!(matches!(
            store.accept_worker_result(&coder, &brief.unit_id, first, &sha('c'), 12),
            Err(StoreError::StaleLease(id)) if id == first
        ));
        assert_eq!(
            store.state(&brief.unit_id).expect("state"),
            JobState::Leased
        );
        store
            .accept_worker_result(&coder, &brief.unit_id, second, &sha('c'), 12)
            .expect("replacement lease remains authoritative");
    }

    struct BoundaryFixture {
        store: ControlStore,
        brief: JobBrief,
        coordinator: ProfileId,
        coder: ProfileId,
        lease_free_coder: ProfileId,
        verifier: ProfileId,
        sibling_coder: ProfileId,
        foreign_coder: ProfileId,
        destroyed: ProfileId,
    }

    fn boundary_fixture() -> BoundaryFixture {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let mut sibling = brief.clone();
        sibling.unit_id = UnitId::new("unit-2").expect("sibling unit");
        store.create_job(&sibling, 1).expect("sibling created");
        let mut foreign = brief.clone();
        foreign.job_id = JobId::new("job-2").expect("foreign job");
        foreign.unit_id = UnitId::new("unit-foreign").expect("foreign unit");
        store.create_job(&foreign, 1).expect("foreign job created");
        let coordinator = ensure_coordinator(&mut store, &brief, 2);
        let coder = ensure_profile(&mut store, &brief, "boundary-coder", Role::Coder, 2);
        let lease_free_coder =
            ensure_profile(&mut store, &brief, "lease-free-coder", Role::Coder, 2);
        let verifier = ensure_profile(
            &mut store,
            &brief,
            "boundary-verifier",
            Role::VerifierReviewer,
            2,
        );
        let sibling_coder = ensure_profile(&mut store, &sibling, "sibling-coder", Role::Coder, 2);
        let foreign_coder = ensure_profile(&mut store, &foreign, "foreign-coder", Role::Coder, 2);
        let destroyed = ensure_profile(&mut store, &brief, "destroyed-coder", Role::Coder, 2);
        store
            .destroy_profile(&coordinator, &destroyed, "destroy-boundary-coder", 2)
            .expect("profile destroyed");
        BoundaryFixture {
            store,
            brief,
            coordinator,
            coder,
            lease_free_coder,
            verifier,
            sibling_coder,
            foreign_coder,
            destroyed,
        }
    }

    fn prepare_boundary_implementing(fixture: &mut BoundaryFixture) -> i64 {
        let store = &mut fixture.store;
        let brief = &fixture.brief;
        store
            .transition(
                &fixture.coordinator,
                &brief.unit_id,
                JobState::Leased,
                "boundary-leased",
                2,
            )
            .expect("unit leased");
        let lease = store
            .acquire_lease(
                &fixture.coordinator,
                &fixture.coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                fixture.coder.as_str(),
                100,
                2,
            )
            .expect("coder lease acquired");
        store
            .register_branch(
                &fixture.coder,
                &brief.unit_id,
                "nswarm/job-1/unit-1",
                std::path::Path::new("/tmp/nswarm-worktrees/unit-1"),
                &brief.base_sha,
                2,
            )
            .expect("coder registers assigned branch");
        for (state, key, now) in [
            (JobState::Grounding, "boundary-grounding", 3),
            (JobState::Implementing, "boundary-implementing", 4),
        ] {
            store
                .transition(&fixture.coder, &brief.unit_id, state, key, now)
                .expect("coding state advances");
        }
        lease
    }

    #[test]
    fn lease_acquisition_rejects_wrong_authority_scope_role_and_liveness() {
        let mut fixture = boundary_fixture();
        let store = &mut fixture.store;
        let brief = &fixture.brief;

        assert!(matches!(
            store.acquire_lease(
                &fixture.coder,
                &fixture.coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                fixture.coder.as_str(),
                100,
                2,
            ),
            Err(StoreError::UnauthorizedCoordinator)
        ));
        for (holder, kind, resource) in [
            (&fixture.verifier, LeaseKind::Path, "src"),
            (
                &fixture.sibling_coder,
                LeaseKind::Profile,
                fixture.sibling_coder.as_str(),
            ),
            (
                &fixture.foreign_coder,
                LeaseKind::Profile,
                fixture.foreign_coder.as_str(),
            ),
            (
                &fixture.destroyed,
                LeaseKind::Profile,
                fixture.destroyed.as_str(),
            ),
        ] {
            assert!(matches!(
                store.acquire_lease(
                    &fixture.coordinator,
                    holder,
                    &brief.job_id,
                    &brief.unit_id,
                    kind,
                    resource,
                    100,
                    2,
                ),
                Err(StoreError::UnauthorizedActor(actor)) if actor == holder.as_str()
            ));
        }
    }

    #[test]
    fn actor_mutations_reject_expired_or_wrong_lease_holders() {
        let mut fixture = boundary_fixture();
        let store = &mut fixture.store;
        let brief = &fixture.brief;
        store
            .transition(
                &fixture.coordinator,
                &brief.unit_id,
                JobState::Leased,
                "boundary-leased",
                2,
            )
            .expect("unit leased");
        let expired_lease = store
            .acquire_lease(
                &fixture.coordinator,
                &fixture.coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                fixture.coder.as_str(),
                3,
                2,
            )
            .expect("short lease acquired");
        assert!(matches!(
            store.transition(
                &fixture.coder,
                &brief.unit_id,
                JobState::Grounding,
                "expired-grounding",
                3,
            ),
            Err(StoreError::MissingActorLease { actor, kind: LeaseKind::Profile })
                if actor == fixture.coder.as_str()
        ));
        let coder_lease = store
            .acquire_lease(
                &fixture.coordinator,
                &fixture.coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                fixture.coder.as_str(),
                100,
                3,
            )
            .expect("replacement coder lease");
        assert_ne!(expired_lease, coder_lease);
        ensure_profile_lease(store, brief, &fixture.verifier, 3);
        let integrator = ensure_profile(store, brief, "boundary-integrator", Role::Integrator, 3);
        let topology_lease = ensure_topology_lease(store, brief, &integrator, 3);
        assert!(matches!(
            store.accept_worker_result(
                &integrator,
                &brief.unit_id,
                topology_lease,
                &sha('b'),
                3,
            ),
            Err(StoreError::StaleLease(id)) if id == topology_lease
        ));

        assert!(matches!(
            store.register_branch(
                &fixture.verifier,
                &brief.unit_id,
                "nswarm/job-1/unit-1",
                std::path::Path::new("/tmp/nswarm-worktrees/unit-1"),
                &brief.base_sha,
                3,
            ),
            Err(StoreError::UnauthorizedActor(actor)) if actor == fixture.verifier.as_str()
        ));
        assert!(matches!(
            store.accept_worker_result(
                &fixture.verifier,
                &brief.unit_id,
                coder_lease,
                &sha('b'),
                3,
            ),
            Err(StoreError::StaleLease(id)) if id == coder_lease
        ));
    }

    #[test]
    fn actor_mutations_reject_wrong_scope_role_and_missing_leases() {
        let mut fixture = boundary_fixture();
        prepare_boundary_implementing(&mut fixture);
        let store = &mut fixture.store;
        let brief = &fixture.brief;
        assert!(matches!(
            store.update_branch_head(
                &fixture.sibling_coder,
                &brief.unit_id,
                &brief.base_sha,
                &sha('b'),
                "wrong-unit-branch-update",
                4,
            ),
            Err(StoreError::UnauthorizedActor(actor)) if actor == fixture.sibling_coder.as_str()
        ));
        store
            .update_branch_head(
                &fixture.coder,
                &brief.unit_id,
                &brief.base_sha,
                &sha('b'),
                "boundary-branch-update",
                4,
            )
            .expect("coder updates assigned branch");

        let valid_report = json!({
            "head_sha": sha('b').as_str(),
            "evidence": {"checks": ["boundary checks passed"]}
        });
        assert!(matches!(
            store.record_report(
                &fixture.foreign_coder,
                &brief.unit_id,
                &valid_report,
                "foreign-report",
                6,
            ),
            Err(StoreError::UnauthorizedActor(actor)) if actor == fixture.foreign_coder.as_str()
        ));
        assert!(matches!(
            store.record_artifact(
                &fixture.lease_free_coder,
                &brief.unit_id,
                ArtifactKind::Log,
                std::path::Path::new("artifacts/no-lease.log"),
                &brief.base_sha,
                &sha('c'),
                6,
            ),
            Err(StoreError::MissingActorLease { actor, kind: LeaseKind::Profile })
                if actor == fixture.lease_free_coder.as_str()
        ));
        store
            .transition(
                &fixture.coder,
                &brief.unit_id,
                JobState::SelfVerifying,
                "boundary-self-verifying",
                7,
            )
            .expect("self verification starts");
        assert!(matches!(
            store.record_candidate(
                &fixture.lease_free_coder,
                &brief.unit_id,
                &sha('b'),
                "lease-free-candidate",
                7,
            ),
            Err(StoreError::MissingActorLease { actor, kind: LeaseKind::Profile })
                if actor == fixture.lease_free_coder.as_str()
        ));
        store
            .record_candidate(
                &fixture.coder,
                &brief.unit_id,
                &sha('b'),
                "boundary-candidate",
                7,
            )
            .expect("leased coder records tracked candidate");
    }

    #[test]
    fn profile_session_branch_and_artifact_repositories_are_scoped() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let profile = ProfileId::new("coder-job-1-unit-1").expect("profile id");
        store
            .register_profile(
                &profile,
                &brief.job_id,
                &brief.unit_id,
                Role::Coder,
                std::path::Path::new("/tmp/nswarm-coder-job-1-unit-1"),
                2,
            )
            .expect("profile registered");
        store
            .register_session(
                &SessionId::new("session-1").expect("session id"),
                &profile,
                "job:job-1:unit:unit-1",
                3,
            )
            .expect("session registered");
        ensure_profile_lease(&mut store, &brief, &profile, 3);
        store
            .register_branch(
                &profile,
                &brief.unit_id,
                "nswarm/job-1/unit-1",
                std::path::Path::new("/tmp/nswarm-worktrees/unit-1"),
                &brief.base_sha,
                4,
            )
            .expect("branch registered");
        let artifact = store
            .record_artifact(
                &profile,
                &brief.unit_id,
                ArtifactKind::TestReport,
                std::path::Path::new("artifacts/report.json"),
                &brief.base_sha,
                &sha('d'),
                5,
            )
            .expect("artifact recorded");
        assert!(artifact > 0);
        assert!(matches!(
            store.record_artifact(
                &profile,
                &brief.unit_id,
                ArtifactKind::Log,
                std::path::Path::new("../sibling/secret.log"),
                &brief.base_sha,
                &sha('e'),
                6,
            ),
            Err(StoreError::InvalidArtifact)
        ));
        assert!(matches!(
            store.record_artifact(
                &profile,
                &brief.unit_id,
                ArtifactKind::Log,
                std::path::Path::new("artifacts/stale.log"),
                &sha('b'),
                &sha('e'),
                6,
            ),
            Err(StoreError::StaleArtifact { current, artifact })
                if current == brief.base_sha && artifact == sha('b')
        ));
        assert!(matches!(
            store.record_review(
                &brief.unit_id,
                &profile,
                &sha('b'),
                ReviewAssessment::Noted,
                &json!({}),
                6,
            ),
            Err(StoreError::ReviewOutsideReviewState)
        ));
    }

    #[test]
    fn identical_artifact_content_is_scoped_to_exact_source_sha() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let artifact_path = std::path::Path::new("artifacts/repeatable.json");
        let digest = sha('d');
        store.create_job(&brief, 1).expect("job created");
        let coder = ensure_profile(&mut store, &brief, "coder-job-1-unit-1", Role::Coder, 2);
        ensure_profile_lease(&mut store, &brief, &coder, 2);
        store
            .record_artifact(
                &coder,
                &brief.unit_id,
                ArtifactKind::TestReport,
                artifact_path,
                &brief.base_sha,
                &digest,
                2,
            )
            .expect("base artifact recorded");
        let coder = advance_to_self_verifying(&mut store, &brief);
        let candidate = sha('b');
        store
            .record_candidate(&coder, &brief.unit_id, &candidate, "artifact-candidate", 7)
            .expect("candidate recorded");
        store
            .record_artifact(
                &coder,
                &brief.unit_id,
                ArtifactKind::TestReport,
                artifact_path,
                &candidate,
                &digest,
                8,
            )
            .expect("same content is distinct evidence for a new SHA");
    }

    #[test]
    fn profile_destruction_revokes_sessions_and_profile_lease() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ProfileId::new("destroy-coordinator").expect("coordinator id");
        let coder = ProfileId::new("destroy-coder").expect("coder id");
        for (profile, role, home) in [
            (
                &coordinator,
                Role::Coordinator,
                "/tmp/nswarm-destroy-coordinator",
            ),
            (&coder, Role::Coder, "/tmp/nswarm-destroy-coder"),
        ] {
            store
                .register_profile(
                    profile,
                    &brief.job_id,
                    &brief.unit_id,
                    role,
                    std::path::Path::new(home),
                    2,
                )
                .expect("profile registered");
        }
        store
            .register_session(
                &SessionId::new("destroy-session").expect("session id"),
                &coder,
                "job:job-1:unit:unit-1",
                3,
            )
            .expect("session registered");
        store
            .acquire_lease(
                &coordinator,
                &coder,
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                coder.as_str(),
                100,
                4,
            )
            .expect("profile lease acquired");
        assert!(matches!(
            store.destroy_profile(&coder, &coder, "self-destroy", 5),
            Err(StoreError::UnauthorizedCoordinator)
        ));
        store
            .destroy_profile(&coordinator, &coder, "coordinator-destroy", 6)
            .expect("coordinator destroys profile authority");
        assert!(matches!(
            store.register_session(
                &SessionId::new("late-session").expect("session id"),
                &coder,
                "late",
                7,
            ),
            Err(StoreError::UnknownLiveProfile(id)) if id == coder.as_str()
        ));
        assert!(matches!(
            store.destroy_profile(&coordinator, &coder, "duplicate-destroy", 8),
            Err(StoreError::UnknownLiveProfile(id)) if id == coder.as_str()
        ));
        let destroyed_sessions: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE profile_id = ?1 AND destroyed_at = 6",
                [coder.as_str()],
                |row| row.get(0),
            )
            .expect("destroyed session count");
        let released_leases: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM leases WHERE resource = ?1 AND released_at = 6",
                [coder.as_str()],
                |row| row.get(0),
            )
            .expect("released profile lease count");
        assert_eq!(destroyed_sessions, 1);
        assert_eq!(released_leases, 1);
    }

    #[test]
    fn only_live_same_job_coordinator_can_revoke_credential_grants() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ProfileId::new("coordinator-job-1").expect("coordinator id");
        let coder = ProfileId::new("coder-job-1").expect("coder id");
        for (profile, role, home) in [
            (
                &coordinator,
                Role::Coordinator,
                "/tmp/nswarm-coordinator-job-1",
            ),
            (&coder, Role::Coder, "/tmp/nswarm-coder-job-1"),
        ] {
            store
                .register_profile(
                    profile,
                    &brief.job_id,
                    &brief.unit_id,
                    role,
                    std::path::Path::new(home),
                    2,
                )
                .expect("profile registered");
        }
        let method = "git:push:refs/heads/nswarm/job-1/unit-1";
        assert!(
            store
                .credential_method_is_active(&brief.job_id, "github-job-push", method,)
                .expect("grant query")
        );
        assert!(matches!(
            store.revoke_credential_grant(
                &brief.job_id,
                &coder,
                "github-job-push",
                "unauthorized-revoke",
                3,
            ),
            Err(StoreError::UnauthorizedCoordinator)
        ));
        store
            .revoke_credential_grant(
                &brief.job_id,
                &coordinator,
                "github-job-push",
                "authorized-revoke",
                4,
            )
            .expect("coordinator revokes");
        store
            .revoke_credential_grant(
                &brief.job_id,
                &coordinator,
                "github-job-push",
                "authorized-revoke",
                99,
            )
            .expect("credential revocation replays after mutation");
        assert!(
            !store
                .credential_method_is_active(&brief.job_id, "github-job-push", method,)
                .expect("revoked grant query")
        );
        assert!(matches!(
            store.revoke_credential_grant(
                &brief.job_id,
                &coordinator,
                "github-job-push",
                "duplicate-revoke",
                5,
            ),
            Err(StoreError::UnknownActiveCredentialGrant(id)) if id == "github-job-push"
        ));
    }

    #[test]
    fn branch_heads_use_cas_and_gate_candidate_sha() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        store.create_job(&brief, 1).expect("job created");
        let coordinator = ensure_coordinator(&mut store, &brief, 2);
        let coder = ensure_profile(
            &mut store,
            &brief,
            "branch-coder-job-1-unit-1",
            Role::Coder,
            2,
        );
        ensure_profile_lease(&mut store, &brief, &coder, 2);
        store
            .register_branch(
                &coder,
                &unit,
                "nswarm/job-1/unit-1",
                std::path::Path::new("/tmp/nswarm-worktrees/unit-1"),
                &sha('a'),
                2,
            )
            .expect("branch registered");
        assert!(matches!(
            store.update_branch_head(&coder, &unit, &sha('a'), &sha('b'), "too-early", 3),
            Err(StoreError::BranchUpdateOutsideCodingState(
                JobState::Pending
            ))
        ));
        store
            .transition(&coordinator, &unit, JobState::Leased, "branch-advance-4", 4)
            .expect("unit leased");
        for (state, timestamp) in [JobState::Grounding, JobState::Implementing]
            .into_iter()
            .zip([5_i64, 6])
        {
            store
                .transition(
                    &coder,
                    &unit,
                    state,
                    &format!("branch-advance-{timestamp}"),
                    timestamp,
                )
                .expect("valid transition");
        }
        assert!(matches!(
            store.update_branch_head(&coder, &unit, &sha('c'), &sha('b'), "stale-head", 7),
            Err(StoreError::StaleBranchHead { current, expected })
                if current == sha('a') && expected == sha('c')
        ));
        store
            .update_branch_head(&coder, &unit, &sha('a'), &sha('b'), "branch-update", 8)
            .expect("head advances");
        store
            .update_branch_head(&coder, &unit, &sha('a'), &sha('b'), "branch-update", 99)
            .expect("branch update replays after head advances");
        assert!(matches!(
            store.update_branch_head(&coder, &unit, &sha('b'), &sha('c'), "branch-update", 9),
            Err(StoreError::IdempotencyConflict(key)) if key == "branch-update"
        ));
        store
            .update_branch_head(&coder, &unit, &sha('b'), &sha('d'), "branch-update-2", 10)
            .expect("conflicting idempotency transaction rolled back");
        store
            .transition(
                &coder,
                &unit,
                JobState::SelfVerifying,
                "self-verify-branch",
                11,
            )
            .expect("self verification starts");
        assert!(matches!(
            store.record_candidate(&coder, &unit, &sha('c'), "stale-candidate", 12),
            Err(StoreError::StaleBranchHead { current, expected })
                if current == sha('d') && expected == sha('c')
        ));
        store
            .record_candidate(&coder, &unit, &sha('d'), "current-candidate", 13)
            .expect("tracked candidate accepted");
    }

    fn assert_schema_column(store: &ControlStore, table: &str, column: &str) {
        let query = format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1");
        let count: i64 = store
            .connection
            .query_row(&query, [column], |row| row.get(0))
            .expect("schema column query");
        assert_eq!(count, 1, "missing {table}.{column}");
    }

    fn assert_current_schema(store: &ControlStore, legacy: &JobBrief) {
        for (table, column) in [
            ("review_findings", "reviewer_profile"),
            ("sessions", "destroyed_at"),
            ("artifacts", "head_sha"),
            ("verification_verdicts", "verifier_profile"),
            ("jobs", "repository"),
            ("jobs", "standing_policy_version"),
            ("merge_authorizations", "invalidated_at"),
            ("leases", "holder_profile"),
        ] {
            assert_schema_column(store, table, column);
        }
        let unit_brief_rows: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM unit_briefs", [], |row| row.get(0))
            .expect("unit brief migration query");
        assert_eq!(unit_brief_rows, 1);
        let migrated_brief: String = store
            .connection
            .query_row(
                "SELECT brief_json FROM unit_briefs WHERE unit_id = ?1",
                [legacy.unit_id.as_str()],
                |row| row.get(0),
            )
            .expect("migrated brief query");
        assert_eq!(
            &serde_json::from_str::<JobBrief>(&migrated_brief).expect("parse migrated brief"),
            legacy
        );
        let dependency_foreign_keys: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_list('dependencies') WHERE \"table\" = 'units'",
                [],
                |row| row.get(0),
            )
            .expect("dependency foreign-key query");
        assert_eq!(dependency_foreign_keys, 2);
        let foreign_keys_enabled: i64 = store
            .connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .expect("foreign keys pragma query");
        assert_eq!(foreign_keys_enabled, 1);
        let foreign_key_violations: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .expect("foreign key check");
        assert_eq!(foreign_key_violations, 0);
        let command_result_table: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'command_results'",
                [],
                |row| row.get(0),
            )
            .expect("command result table query");
        assert_eq!(command_result_table, 1);
        let schema_version: i64 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version query");
        assert_eq!(schema_version, super::SCHEMA_VERSION);
    }

    #[test]
    fn populated_v1_through_v8_schemas_migrate_idempotently() {
        let legacy = brief();
        for legacy_version in 1_i64..=8 {
            let connection = rusqlite::Connection::open_in_memory().expect("connection");
            connection.execute_batch(SCHEMA).expect("v1 schema");
            connection
                .execute(
                    "INSERT INTO jobs (job_id, brief_json, created_at) VALUES (?1, ?2, 1)",
                    rusqlite::params![
                        legacy.job_id.as_str(),
                        serde_json::to_string(&legacy).expect("serialize legacy brief")
                    ],
                )
                .expect("legacy job inserted");
            connection
                .execute(
                    "INSERT INTO units (unit_id, job_id, state, base_sha, updated_at) VALUES (?1, ?2, 'pending', ?3, 1)",
                    rusqlite::params![
                        legacy.unit_id.as_str(),
                        legacy.job_id.as_str(),
                        legacy.base_sha.as_str()
                    ],
                )
                .expect("legacy unit inserted");
            for (version, migration) in [
                (2_i64, MIGRATION_2),
                (3_i64, MIGRATION_3),
                (4_i64, MIGRATION_4),
                (5_i64, MIGRATION_5),
                (6_i64, MIGRATION_6),
                (7_i64, MIGRATION_7),
                (8_i64, MIGRATION_8),
            ] {
                if version > legacy_version {
                    break;
                }
                connection
                    .execute_batch(migration)
                    .expect("legacy migration applied");
            }
            if legacy_version == 6 {
                connection
                    .execute(
                        "INSERT INTO profiles (profile_id, job_id, unit_id, role, home) VALUES ('legacy-shipper', ?1, ?2, 'shipper', '/tmp/legacy-shipper')",
                        rusqlite::params![legacy.job_id.as_str(), legacy.unit_id.as_str()],
                    )
                    .expect("legacy shipper inserted");
                connection
                    .execute(
                        "INSERT INTO merge_authorizations (unit_id, head_sha, authorized_by, created_at) VALUES (?1, ?2, 'legacy-shipper', 2)",
                        rusqlite::params![legacy.unit_id.as_str(), sha('b').as_str()],
                    )
                    .expect("legacy authorization inserted");
            }
            if legacy_version == 7 {
                connection
                    .execute(
                        "INSERT INTO profiles (profile_id, job_id, unit_id, role, home) VALUES ('legacy-coder', ?1, ?2, 'coder', '/tmp/legacy-coder')",
                        rusqlite::params![legacy.job_id.as_str(), legacy.unit_id.as_str()],
                    )
                    .expect("legacy coder inserted");
                connection
                    .execute(
                        "INSERT INTO leases (job_id, unit_id, kind, resource, expires_at) VALUES (?1, ?2, 'profile', 'legacy-coder', 100)",
                        rusqlite::params![legacy.job_id.as_str(), legacy.unit_id.as_str()],
                    )
                    .expect("legacy holderless lease inserted");
            }
            connection
                .pragma_update(None, "user_version", legacy_version)
                .expect("set legacy version");
            let mut store = ControlStore { connection };
            store.migrate().expect("legacy to current migration");
            store.migrate().expect("current migration is idempotent");
            assert_current_schema(&store, &legacy);
            if legacy_version == 6 {
                let authorization: (String, String, Option<i64>) = store
                    .connection
                    .query_row(
                        "SELECT head_sha, authorized_by, invalidated_at FROM merge_authorizations WHERE unit_id = ?1",
                        [legacy.unit_id.as_str()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .expect("migrated authorization query");
                assert_eq!(
                    authorization,
                    (sha('b').to_string(), "legacy-shipper".to_owned(), None)
                );
            }
            if legacy_version == 7 {
                let holder: Option<String> = store
                    .connection
                    .query_row(
                        "SELECT holder_profile FROM leases WHERE resource = 'legacy-coder'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("migrated holder query");
                assert_eq!(holder, None, "legacy leases fail closed after migration");
            }
        }
    }
}
