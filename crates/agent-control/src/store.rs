use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use thiserror::Error;

use crate::types::BriefError;
use crate::{
    JobBrief, JobId, JobState, LeaseKind, ProfileId, RiskClass, Role, SessionId, Sha, UnitId,
};

const SCHEMA_VERSION: i64 = 3;

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
    pub fn migrate(&mut self) -> Result<(), StoreError> {
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
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        }
        Ok(())
    }

    /// Stores one validated immutable brief and its dependent records.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when validation, serialization, or the atomic
    /// insert fails.
    pub fn create_job(&mut self, brief: &JobBrief, now: i64) -> Result<(), StoreError> {
        brief.validate()?;
        let brief_json = serde_json::to_string(brief)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO jobs (job_id, brief_json, created_at) VALUES (?1, ?2, ?3)",
            params![brief.job_id.as_str(), brief_json, now],
        )?;
        transaction.execute(
            "INSERT INTO units (unit_id, job_id, state, base_sha, updated_at) VALUES (?1, ?2, 'pending', ?3, ?4)",
            params![
                brief.unit_id.as_str(),
                brief.job_id.as_str(),
                brief.base_sha.as_str(),
                now
            ],
        )?;
        for dependency in &brief.dependencies {
            transaction.execute(
                "INSERT INTO dependencies (unit_id, depends_on_unit_id) VALUES (?1, ?2)",
                params![brief.unit_id.as_str(), dependency.as_str()],
            )?;
        }
        for grant in &brief.credential_grants {
            transaction.execute(
                "INSERT INTO credential_grants (job_id, credential_id, methods_json, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    brief.job_id.as_str(),
                    grant.credential_id,
                    serde_json::to_string(&grant.methods)?,
                    now
                ],
            )?;
        }
        append_event_tx(
            &transaction,
            &brief.job_id,
            &format!("job-created:{}", brief.unit_id),
            "job-created",
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
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if !current.can_transition_to(next) {
            return Err(StoreError::InvalidTransition { current, next });
        }
        update_state_tx(&transaction, unit_id, next, now)?;
        append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "state-transition",
            &json!({
                "unit_id": unit_id.as_str(),
                "from": current.as_str(),
                "to": next.as_str()
            }),
            now,
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
        unit_id: &UnitId,
        head_sha: &Sha,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::SelfVerifying {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::CandidateReady,
            });
        }
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
        append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "candidate-recorded",
            &json!({"unit_id": unit_id.as_str(), "head_sha": head_sha.as_str()}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Publishes an exact-SHA verification verdict and enters review.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for stale SHA evidence or the wrong state.
    pub fn record_verdict(
        &mut self,
        unit_id: &UnitId,
        head_sha: &Sha,
        passed: bool,
        evidence: &Value,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::IndependentlyVerifying {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::Reviewing,
            });
        }
        let candidate = candidate_sha_tx(&transaction, unit_id)?;
        if candidate != *head_sha {
            return Err(StoreError::StaleVerification {
                candidate,
                verdict: head_sha.clone(),
            });
        }
        transaction.execute(
            "INSERT INTO verification_verdicts (unit_id, head_sha, passed, evidence_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                unit_id.as_str(),
                head_sha.as_str(),
                passed,
                serde_json::to_string(&redact_evidence(evidence))?,
                now
            ],
        )?;
        update_state_tx(&transaction, unit_id, JobState::Reviewing, now)?;
        append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "verification-recorded",
            &json!({
                "unit_id": unit_id.as_str(),
                "head_sha": head_sha.as_str(),
                "passed": passed
            }),
            now,
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
    pub fn accept_verdict(
        &mut self,
        unit_id: &UnitId,
        idempotency_key: &str,
        now: i64,
    ) -> Result<JobState, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::Reviewing {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::Verified,
            });
        }
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
        append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "verdict-accepted",
            &json!({"unit_id": unit_id.as_str(), "head_sha": candidate.as_str(), "state": next.as_str()}),
            now,
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
        unit_id: &UnitId,
        integrated_sha: &Sha,
        idempotency_key: &str,
        now: i64,
    ) -> Result<JobState, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::Integrating {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::Integrated,
            });
        }
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
        append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "integration-completed",
            &json!({
                "unit_id": unit_id.as_str(),
                "old_sha": candidate.as_str(),
                "integrated_sha": integrated_sha.as_str(),
                "requires_reverification": next == JobState::CandidateReady
            }),
            now,
        )?;
        transaction.commit()?;
        Ok(next)
    }

    /// Grants one exact-SHA merge authorization after integration verification.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for an unverified, stale, or unauthorized SHA.
    pub fn authorize_merge(
        &mut self,
        unit_id: &UnitId,
        head_sha: &Sha,
        authorized_by: &str,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        if authorized_by.trim().is_empty() {
            return Err(StoreError::MissingAuthorizer);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::Integrated {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::MergeAuthorized,
            });
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
            params![unit_id.as_str(), head_sha.as_str(), authorized_by, now],
        )?;
        update_state_tx(&transaction, unit_id, JobState::MergeAuthorized, now)?;
        append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "merge-authorized",
            &json!({"unit_id": unit_id.as_str(), "head_sha": head_sha.as_str(), "authorized_by": authorized_by}),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Records protected-branch completion for exactly the authorized SHA.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for a stale SHA or missing authorization.
    pub fn record_merged(
        &mut self,
        unit_id: &UnitId,
        head_sha: &Sha,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        if current != JobState::MergeAuthorized {
            return Err(StoreError::InvalidTransition {
                current,
                next: JobState::Merged,
            });
        }
        let authorized: Option<String> = transaction
            .query_row(
                "SELECT head_sha FROM merge_authorizations WHERE unit_id = ?1",
                [unit_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if authorized.as_deref() != Some(head_sha.as_str()) {
            let expected = authorized
                .map(Sha::new)
                .transpose()?
                .ok_or(StoreError::MissingMergeAuthorization)?;
            return Err(StoreError::UnauthorizedSha {
                expected,
                actual: head_sha.clone(),
            });
        }
        update_state_tx(&transaction, unit_id, JobState::Merged, now)?;
        append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "merged",
            &json!({"unit_id": unit_id.as_str(), "head_sha": head_sha.as_str()}),
            now,
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
    pub fn acquire_lease(
        &mut self,
        job_id: &JobId,
        unit_id: &UnitId,
        kind: LeaseKind,
        resource: &str,
        expires_at: i64,
        now: i64,
    ) -> Result<i64, StoreError> {
        if resource.trim().is_empty() || expires_at <= now {
            return Err(StoreError::InvalidLease);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (actual_job, _) = unit_identity_tx(&transaction, unit_id)?;
        if actual_job != *job_id {
            return Err(StoreError::JobUnitMismatch);
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
            "SELECT resource FROM leases WHERE kind = ?1 AND released_at IS NULL AND expires_at > ?2",
        )?;
        let resources = statement
            .query_map(params![kind.as_str(), now], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let overlaps = match kind {
            LeaseKind::Path => resources
                .iter()
                .any(|active| path_resources_overlap(active, resource)),
            LeaseKind::Topology => !resources.is_empty(),
            LeaseKind::Profile => resources.iter().any(|active| active == resource),
        };
        if overlaps {
            return Err(StoreError::LeaseConflict(resource.to_owned()));
        }
        transaction.execute(
            "INSERT INTO leases (job_id, unit_id, kind, resource, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![job_id.as_str(), unit_id.as_str(), kind.as_str(), resource, expires_at],
        )?;
        let lease_id = transaction.last_insert_rowid();
        append_event_tx(
            &transaction,
            job_id,
            &format!("lease-acquired:{lease_id}"),
            "lease-acquired",
            &json!({"unit_id": unit_id.as_str(), "lease_id": lease_id, "kind": kind.as_str(), "resource": resource}),
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
    pub fn accept_worker_result(
        &mut self,
        unit_id: &UnitId,
        lease_id: i64,
        head_sha: &Sha,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, current) = unit_identity_tx(&transaction, unit_id)?;
        let live: bool = transaction
            .query_row(
                "SELECT expires_at > ?1 AND released_at IS NULL FROM leases WHERE lease_id = ?2 AND unit_id = ?3",
                params![now, lease_id, unit_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false);
        if !live {
            if current.can_transition_to(JobState::Quarantined) {
                update_state_tx(&transaction, unit_id, JobState::Quarantined, now)?;
            }
            append_event_tx(
                &transaction,
                &job_id,
                &format!("stale-result:{lease_id}:{}", head_sha.as_str()),
                "result-quarantined",
                &json!({"unit_id": unit_id.as_str(), "lease_id": lease_id, "head_sha": head_sha.as_str()}),
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
            &json!({"unit_id": unit_id.as_str(), "lease_id": lease_id, "head_sha": head_sha.as_str()}),
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
    pub fn append_event(
        &mut self,
        job_id: &JobId,
        idempotency_key: &str,
        event_type: &str,
        payload: &Value,
        now: i64,
    ) -> Result<i64, StoreError> {
        if idempotency_key.trim().is_empty() || event_type.trim().is_empty() {
            return Err(StoreError::InvalidEvent);
        }
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
        unit_id: &UnitId,
        report: &Value,
        idempotency_key: &str,
        now: i64,
    ) -> Result<i64, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, _) = unit_identity_tx(&transaction, unit_id)?;
        let brief_json: String = transaction.query_row(
            "SELECT brief_json FROM jobs WHERE job_id = ?1",
            [job_id.as_str()],
            |row| row.get(0),
        )?;
        let brief: JobBrief = serde_json::from_str(&brief_json)?;
        let report_object = report
            .as_object()
            .ok_or(StoreError::ReportSchemaViolation)?;
        let required = brief
            .report_schema
            .get("required")
            .and_then(Value::as_array)
            .ok_or(StoreError::ReportSchemaViolation)?;
        if required.iter().any(|field| {
            field
                .as_str()
                .is_none_or(|name| !report_object.contains_key(name))
        }) {
            return Err(StoreError::ReportSchemaViolation);
        }
        let id = append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "worker-report",
            report,
            now,
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
    pub fn register_profile(
        &mut self,
        profile_id: &ProfileId,
        job_id: &JobId,
        unit_id: &UnitId,
        role: Role,
        home: &Path,
        now: i64,
    ) -> Result<(), StoreError> {
        if !home.is_absolute() {
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
    pub fn register_session(
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
        let target_record: Option<(String, String)> = transaction
            .query_row(
                "SELECT job_id, unit_id FROM profiles WHERE profile_id = ?1 AND destroyed_at IS NULL",
                [target.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (job_id, unit_id) =
            target_record.ok_or_else(|| StoreError::UnknownLiveProfile(target.to_string()))?;
        let authorized: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM profiles WHERE profile_id = ?1 AND job_id = ?2 AND role = ?3 AND destroyed_at IS NULL",
                params![coordinator.as_str(), &job_id, Role::Coordinator.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if authorized.is_none() {
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
            "UPDATE leases SET released_at = ?1 WHERE job_id = ?2 AND unit_id = ?3 AND kind = 'profile' AND resource = ?4 AND released_at IS NULL",
            params![now, &job_id, &unit_id, target.as_str()],
        )?;
        append_event_tx(
            &transaction,
            &JobId::new(job_id)?,
            idempotency_key,
            "profile-destroyed",
            &json!({
                "profile_id": target.as_str(),
                "coordinator": coordinator.as_str(),
                "unit_id": unit_id
            }),
            now,
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
        let authorized: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM profiles WHERE profile_id = ?1 AND job_id = ?2 AND role = ?3 AND destroyed_at IS NULL",
                params![coordinator.as_str(), job_id.as_str(), Role::Coordinator.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if authorized.is_none() {
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
        append_event_tx(
            &transaction,
            job_id,
            idempotency_key,
            "credential-revoked",
            &json!({"credential_id": credential_id, "coordinator": coordinator.as_str()}),
            now,
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
    pub fn register_branch(
        &mut self,
        unit_id: &UnitId,
        name: &str,
        worktree: &Path,
        base_sha: &Sha,
        now: i64,
    ) -> Result<(), StoreError> {
        if !worktree.is_absolute() {
            return Err(StoreError::InvalidWorktree(worktree.to_path_buf()));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, _) = unit_identity_tx(&transaction, unit_id)?;
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
            &json!({"unit_id": unit_id.as_str(), "name": name, "base_sha": base_sha.as_str()}),
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
    pub fn update_branch_head(
        &mut self,
        unit_id: &UnitId,
        expected_head: &Sha,
        new_head: &Sha,
        idempotency_key: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, state) = unit_identity_tx(&transaction, unit_id)?;
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
        append_event_tx(
            &transaction,
            &job_id,
            idempotency_key,
            "branch-head-updated",
            &json!({
                "unit_id": unit_id.as_str(),
                "previous_head": expected_head.as_str(),
                "head_sha": new_head.as_str()
            }),
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Stores an immutable artifact digest for later verification.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for blank kind/path or database failure.
    pub fn record_artifact(
        &mut self,
        unit_id: &UnitId,
        kind: &str,
        path: &Path,
        digest: &Sha,
        now: i64,
    ) -> Result<i64, StoreError> {
        if kind.trim().is_empty() || path.as_os_str().is_empty() {
            return Err(StoreError::InvalidArtifact);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (job_id, _) = unit_identity_tx(&transaction, unit_id)?;
        transaction.execute(
            "INSERT INTO artifacts (unit_id, kind, path, digest, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![unit_id.as_str(), kind, path.to_string_lossy(), digest.as_str(), now],
        )?;
        let artifact_id = transaction.last_insert_rowid();
        append_event_tx(
            &transaction,
            &job_id,
            &format!("artifact-recorded:{artifact_id}"),
            "artifact-recorded",
            &json!({"unit_id": unit_id.as_str(), "artifact_id": artifact_id, "digest": digest.as_str()}),
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
        let reviewer_record: Option<(String, String)> = transaction
            .query_row(
                "SELECT job_id, role FROM profiles WHERE profile_id = ?1 AND destroyed_at IS NULL",
                [reviewer.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if !reviewer_record.as_ref().is_some_and(|(review_job, role)| {
            review_job == job_id.as_str() && role == Role::VerifierReviewer.as_str()
        }) {
            return Err(StoreError::UnauthorizedReviewer);
        }
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
        let (job_id, state) = unit_identity_tx(&transaction, unit_id)?;
        if state != JobState::Reviewing {
            return Err(StoreError::ReviewOutsideReviewState);
        }
        let authorized: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM profiles WHERE profile_id = ?1 AND job_id = ?2 AND role = ?3 AND destroyed_at IS NULL",
                params![integrator.as_str(), job_id.as_str(), Role::Integrator.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if authorized.is_none() {
            return Err(StoreError::UnauthorizedIntegrator);
        }
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
        append_event_tx(
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
        transaction.commit()?;
        Ok(())
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

fn require_passing_verdict_tx(
    transaction: &rusqlite::Transaction<'_>,
    unit_id: &UnitId,
    head_sha: &Sha,
) -> Result<(), StoreError> {
    let passed: Option<bool> = transaction
        .query_row(
            "SELECT passed FROM verification_verdicts WHERE unit_id = ?1 AND head_sha = ?2 ORDER BY verdict_id DESC LIMIT 1",
            params![unit_id.as_str(), head_sha.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if passed == Some(true) {
        Ok(())
    } else {
        Err(StoreError::MissingPassingVerdict(head_sha.clone()))
    }
}

fn require_review_gate_tx(
    transaction: &rusqlite::Transaction<'_>,
    unit_id: &UnitId,
    head_sha: &Sha,
) -> Result<(), StoreError> {
    let brief_json: String = transaction.query_row(
        "SELECT jobs.brief_json FROM jobs JOIN units USING (job_id) WHERE units.unit_id = ?1",
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

fn redact_evidence(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, item)| {
                    let upper = key.to_ascii_uppercase();
                    let secret_field = [
                        "KEY",
                        "TOKEN",
                        "PASSWORD",
                        "SECRET",
                        "CREDENTIAL",
                        "AUTHORIZATION",
                        "COOKIE",
                    ]
                    .iter()
                    .any(|name| upper == *name || upper.ends_with(&format!("_{name}")));
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

fn contains_secret_shape(text: &str) -> bool {
    text.to_ascii_uppercase().contains("PRIVATE KEY")
        || text
            .split(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
            })
            .any(|word| {
                (word.starts_with("ghp_") && word.len() >= 30)
                    || (word.starts_with("sk-") && word.len() >= 20)
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
    /// Authorization must be attributable.
    #[error("merge authorizer must not be empty")]
    MissingAuthorizer,
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
    /// Profile home must be an explicit isolated absolute path.
    #[error("profile home must be absolute: {path}", path = .0.display())]
    InvalidProfileHome(std::path::PathBuf),
    /// Profile registration cannot cross job ownership.
    #[error("job and unit ownership do not match")]
    JobUnitMismatch,
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
    /// Artifact kind and path are required evidence fields.
    #[error("artifact kind and path must not be empty")]
    InvalidArtifact,
    /// Reviews are accepted only during the explicit review state.
    #[error("review was submitted outside the reviewing state")]
    ReviewOutsideReviewState,
    /// Only an isolated verifier/reviewer profile for the same job may review.
    #[error("profile is not an authorized independent reviewer")]
    UnauthorizedReviewer,
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{ControlStore, FindingDisposition, ReviewAssessment, SCHEMA, StoreError};
    use crate::{
        CredentialGrant, JobBrief, JobId, JobState, LeaseKind, NetworkMode, NetworkPolicy,
        PathPolicy, ProfileId, ResourceLimits, RiskClass, Role, SessionId, Sha, UnitId,
        VerificationCommand,
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
            report_schema: json!({"type": "object", "required": ["head_sha", "evidence"]}),
            standing_policy_version: "v1".to_owned(),
        }
    }

    fn advance_to_self_verifying(store: &mut ControlStore, unit: &UnitId) {
        for (state, timestamp) in [
            JobState::Leased,
            JobState::Grounding,
            JobState::Implementing,
            JobState::SelfVerifying,
        ]
        .into_iter()
        .zip([2_i64, 3, 4, 5])
        {
            store
                .transition(unit, state, &format!("advance-{timestamp}"), timestamp)
                .expect("valid transition");
        }
    }

    fn prepare_reviewing_candidate(store: &mut ControlStore, brief: &JobBrief, candidate: &Sha) {
        store.create_job(brief, 1).expect("job created");
        advance_to_self_verifying(store, &brief.unit_id);
        store
            .record_candidate(&brief.unit_id, candidate, "candidate-prepared", 7)
            .expect("candidate recorded");
        store
            .transition(
                &brief.unit_id,
                JobState::IndependentlyVerifying,
                "verification-prepared",
                8,
            )
            .expect("verification starts");
        store
            .record_verdict(
                &brief.unit_id,
                candidate,
                true,
                &json!({"commands": ["cargo test"]}),
                "verdict-prepared",
                9,
            )
            .expect("verdict recorded");
    }

    fn record_two_reviews(store: &mut ControlStore, brief: &JobBrief, head_sha: &Sha, now: i64) {
        let integrator = ProfileId::new("integrator-review-gate").expect("valid integrator");
        store
            .register_profile(
                &integrator,
                &brief.job_id,
                &brief.unit_id,
                Role::Integrator,
                PathBuf::from("/tmp/nswarm-integrator-review-gate").as_path(),
                now,
            )
            .expect("integrator profile registered");
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
            store
                .dispose_review_finding(
                    &brief.unit_id,
                    &integrator,
                    &FindingDisposition {
                        finding_id,
                        disposition: ReviewAssessment::Noted,
                        rationale: &json!({"reason": "independent review evidence accepted"}),
                        idempotency_key: &format!("review-disposed:{finding_id}"),
                    },
                    now + index + 4,
                )
                .expect("review disposed");
        }
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
    fn missing_brief_fields_refuse_creation() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let mut invalid = brief();
        invalid.verification_commands.clear();
        assert!(store.create_job(&invalid, 1).is_err());
    }

    #[test]
    fn candidate_requires_commit_sha_and_independent_proof() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        store.create_job(&brief, 1).expect("job created");
        assert!(matches!(
            store.transition(&unit, JobState::Verified, "skip", 2),
            Err(StoreError::DedicatedGateRequired(JobState::Verified))
        ));
        advance_to_self_verifying(&mut store, &unit);
        store
            .record_candidate(&unit, &sha('b'), "candidate", 7)
            .expect("candidate recorded");
        assert_eq!(store.state(&unit).expect("state"), JobState::CandidateReady);
    }

    #[test]
    fn changed_sha_invalidates_verification() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        store.create_job(&brief, 1).expect("job created");
        advance_to_self_verifying(&mut store, &unit);
        store
            .record_candidate(&unit, &sha('b'), "candidate", 7)
            .expect("candidate recorded");
        store
            .transition(&unit, JobState::IndependentlyVerifying, "verify", 8)
            .expect("verification starts");
        assert!(matches!(
            store.record_verdict(&unit, &sha('c'), true, &json!({}), "stale", 9),
            Err(StoreError::StaleVerification { .. })
        ));
    }

    #[test]
    fn integration_content_change_requires_reverification() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        store.create_job(&brief, 1).expect("job created");
        advance_to_self_verifying(&mut store, &unit);
        store
            .record_candidate(&unit, &sha('b'), "candidate", 7)
            .expect("candidate recorded");
        store
            .transition(&unit, JobState::IndependentlyVerifying, "verify", 8)
            .expect("verification starts");
        store
            .record_verdict(
                &unit,
                &sha('b'),
                true,
                &json!({"commands": ["cargo test"]}),
                "verdict",
                9,
            )
            .expect("verdict recorded");
        assert!(matches!(
            store.accept_verdict(&unit, "premature-accept", 10),
            Err(StoreError::ReviewGateUnsatisfied { reviewers: 0, .. })
        ));
        record_two_reviews(&mut store, &brief, &sha('b'), 10);
        assert_eq!(
            store.accept_verdict(&unit, "accept", 15).expect("accepted"),
            JobState::Verified
        );
        store
            .transition(&unit, JobState::Integrating, "integrate", 16)
            .expect("integration starts");
        assert_eq!(
            store
                .complete_integration(&unit, &sha('c'), "integrated", 17)
                .expect("integration completes"),
            JobState::CandidateReady
        );
        assert!(matches!(
            store.authorize_merge(&unit, &sha('c'), "owner", "authorize", 18),
            Err(StoreError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn overlapping_and_topology_leases_are_rejected() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        store
            .acquire_lease(
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
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Path,
                "crates/assigned/src",
                100,
                3,
            ),
            Err(StoreError::LeaseConflict(_))
        ));
        store
            .acquire_lease(
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
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Topology,
                "other-integration-stack",
                100,
                4,
            ),
            Err(StoreError::LeaseConflict(_))
        ));
    }

    #[test]
    fn zombie_result_is_durably_quarantined() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        store
            .transition(&brief.unit_id, JobState::Leased, "leased", 2)
            .expect("leased");
        let lease = store
            .acquire_lease(
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                "coder-job-1-unit-1",
                10,
                2,
            )
            .expect("lease acquired");
        assert!(matches!(
            store.accept_worker_result(&brief.unit_id, lease, &sha('b'), 11),
            Err(StoreError::StaleLease(_))
        ));
        assert_eq!(
            store.state(&brief.unit_id).expect("state"),
            JobState::Quarantined
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
    }

    #[test]
    fn worker_reports_are_schema_checked_and_redacted_before_storage() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        assert!(matches!(
            store.record_report(
                &brief.unit_id,
                &json!({"head_sha": sha('b').as_str()}),
                "incomplete-report",
                2,
            ),
            Err(StoreError::ReportSchemaViolation)
        ));

        let provider_token = "sk-".to_owned() + &"x".repeat(24);
        let source_token = "ghp_".to_owned() + &"y".repeat(30);
        store
            .record_report(
                &brief.unit_id,
                &json!({
                    "head_sha": sha('b').as_str(),
                    "evidence": {
                        "OPENROUTER_API_KEY": provider_token,
                        "nested": [{"authorization": source_token}],
                        "note": "focused test passed"
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
        assert_eq!(stored.matches("[REDACTED]").count(), 2);
        assert!(stored.contains("focused test passed"));
    }

    #[test]
    fn verdict_and_review_evidence_are_redacted_before_storage() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        let candidate = sha('b');
        store.create_job(&brief, 1).expect("job created");
        advance_to_self_verifying(&mut store, &unit);
        store
            .record_candidate(&unit, &candidate, "candidate-redaction", 7)
            .expect("candidate recorded");
        store
            .transition(
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
                &candidate,
                true,
                &json!({"provider_token": token, "result": "pass"}),
                "verdict-redaction",
                9,
            )
            .expect("verdict recorded");
        let reviewer = ProfileId::new("reviewer-redaction").expect("reviewer id");
        store
            .register_profile(
                &reviewer,
                &brief.job_id,
                &unit,
                Role::VerifierReviewer,
                std::path::Path::new("/tmp/nswarm-reviewer-redaction"),
                10,
            )
            .expect("reviewer registered");
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
    }

    #[test]
    fn exact_sha_can_complete_the_full_authorized_lifecycle() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        let candidate = sha('b');
        store.create_job(&brief, 1).expect("job created");
        advance_to_self_verifying(&mut store, &unit);
        store
            .record_candidate(&unit, &candidate, "candidate-full", 7)
            .expect("candidate recorded");
        store
            .transition(&unit, JobState::IndependentlyVerifying, "verify-full", 8)
            .expect("verification starts");
        store
            .record_verdict(
                &unit,
                &candidate,
                true,
                &json!({"commands": ["cargo test"], "artifacts": []}),
                "verdict-full",
                9,
            )
            .expect("verdict recorded");
        record_two_reviews(&mut store, &brief, &candidate, 10);
        assert_eq!(
            store
                .accept_verdict(&unit, "accept-full", 15)
                .expect("accepted"),
            JobState::Verified
        );
        store
            .transition(&unit, JobState::Integrating, "integrate-full", 16)
            .expect("integration starts");
        assert_eq!(
            store
                .complete_integration(&unit, &candidate, "integrated-full", 17)
                .expect("unchanged integration remains verified"),
            JobState::Integrated
        );
        assert!(matches!(
            store.authorize_merge(&unit, &sha('c'), "owner", "wrong-auth", 18),
            Err(StoreError::UnauthorizedSha { .. })
        ));
        store
            .authorize_merge(&unit, &candidate, "owner", "authorize-full", 19)
            .expect("exact SHA authorized");
        assert!(matches!(
            store.record_merged(&unit, &sha('c'), "wrong-merge", 20),
            Err(StoreError::UnauthorizedSha { .. })
        ));
        store
            .record_merged(&unit, &candidate, "merged-full", 21)
            .expect("exact SHA merged");
        assert_eq!(store.state(&unit).expect("state"), JobState::Merged);
    }

    #[test]
    fn review_gate_requires_live_integrator_disposition() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        let unit = brief.unit_id.clone();
        let candidate = sha('b');
        prepare_reviewing_candidate(&mut store, &brief, &candidate);

        let (reviewers, findings) = record_unresolved_reviews(&mut store, &brief, &candidate);
        assert!(matches!(
            store.accept_verdict(&unit, "unresolved-accept", 15),
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

        let integrator = ProfileId::new("disposition-integrator").expect("integrator id");
        store
            .register_profile(
                &integrator,
                &brief.job_id,
                &unit,
                Role::Integrator,
                std::path::Path::new("/tmp/nswarm-disposition-integrator"),
                17,
            )
            .expect("integrator registered");
        for (index, finding) in findings.into_iter().enumerate() {
            store
                .dispose_review_finding(
                    &unit,
                    &integrator,
                    &FindingDisposition {
                        finding_id: finding,
                        disposition: ReviewAssessment::Noted,
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
        assert_eq!(
            store
                .accept_verdict(&unit, "disposed-accept", 22)
                .expect("disposed reviews accepted"),
            JobState::Verified
        );
    }

    #[test]
    fn live_worker_result_and_expired_lease_replacement_are_explicit() {
        let mut store = ControlStore::open_in_memory().expect("store opens");
        let brief = brief();
        store.create_job(&brief, 1).expect("job created");
        store
            .transition(&brief.unit_id, JobState::Leased, "leased-live", 2)
            .expect("leased");
        let first = store
            .acquire_lease(
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                "coder-job-1-unit-1",
                10,
                2,
            )
            .expect("first lease");
        store
            .accept_worker_result(&brief.unit_id, first, &sha('b'), 5)
            .expect("live result accepted");
        let second = store
            .acquire_lease(
                &brief.job_id,
                &brief.unit_id,
                LeaseKind::Profile,
                "coder-job-1-unit-1",
                20,
                11,
            )
            .expect("expired lease is closed before replacement");
        assert_ne!(first, second);
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
        store
            .register_branch(
                &brief.unit_id,
                "nswarm/job-1/unit-1",
                std::path::Path::new("/tmp/nswarm-worktrees/unit-1"),
                &brief.base_sha,
                4,
            )
            .expect("branch registered");
        let artifact = store
            .record_artifact(
                &brief.unit_id,
                "test-report",
                std::path::Path::new("artifacts/report.json"),
                &sha('d'),
                5,
            )
            .expect("artifact recorded");
        assert!(artifact > 0);
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
        store
            .register_branch(
                &unit,
                "nswarm/job-1/unit-1",
                std::path::Path::new("/tmp/nswarm-worktrees/unit-1"),
                &sha('a'),
                2,
            )
            .expect("branch registered");
        assert!(matches!(
            store.update_branch_head(&unit, &sha('a'), &sha('b'), "too-early", 3),
            Err(StoreError::BranchUpdateOutsideCodingState(
                JobState::Pending
            ))
        ));
        for (state, timestamp) in [
            JobState::Leased,
            JobState::Grounding,
            JobState::Implementing,
        ]
        .into_iter()
        .zip([4_i64, 5, 6])
        {
            store
                .transition(
                    &unit,
                    state,
                    &format!("branch-advance-{timestamp}"),
                    timestamp,
                )
                .expect("valid transition");
        }
        assert!(matches!(
            store.update_branch_head(&unit, &sha('c'), &sha('b'), "stale-head", 7),
            Err(StoreError::StaleBranchHead { current, expected })
                if current == sha('a') && expected == sha('c')
        ));
        store
            .update_branch_head(&unit, &sha('a'), &sha('b'), "branch-update", 8)
            .expect("head advances");
        assert!(matches!(
            store.update_branch_head(&unit, &sha('b'), &sha('c'), "branch-update", 9),
            Err(StoreError::IdempotencyConflict(key)) if key == "branch-update"
        ));
        store
            .update_branch_head(&unit, &sha('b'), &sha('d'), "branch-update-2", 10)
            .expect("conflicting idempotency transaction rolled back");
        store
            .transition(&unit, JobState::SelfVerifying, "self-verify-branch", 11)
            .expect("self verification starts");
        assert!(matches!(
            store.record_candidate(&unit, &sha('c'), "stale-candidate", 12),
            Err(StoreError::StaleBranchHead { current, expected })
                if current == sha('d') && expected == sha('c')
        ));
        store
            .record_candidate(&unit, &sha('d'), "current-candidate", 13)
            .expect("tracked candidate accepted");
    }

    #[test]
    fn prior_schema_migrates_to_reviewer_attribution() {
        let connection = rusqlite::Connection::open_in_memory().expect("connection");
        connection.execute_batch(SCHEMA).expect("v1 schema");
        connection
            .pragma_update(None, "user_version", 1_i64)
            .expect("set v1");
        let mut store = ControlStore { connection };
        store.migrate().expect("v1 to v2 migration");
        let reviewer_column: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('review_findings') WHERE name = 'reviewer_profile'",
                [],
                |row| row.get(0),
            )
            .expect("column query");
        assert_eq!(reviewer_column, 1);
        let destroyed_session_column: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'destroyed_at'",
                [],
                |row| row.get(0),
            )
            .expect("session column query");
        assert_eq!(destroyed_session_column, 1);
    }
}
