//! Machine-validated evidence reports for the Step 5 research profile.

use std::collections::BTreeSet;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::JobBrief;

/// Evidence classification required for every research claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimKind {
    /// Directly supported by the cited source.
    Direct,
    /// Reasoned from cited evidence rather than stated by it.
    Inferred,
    /// Evidence that conflicts with another claim or source.
    Contradicted,
    /// Not answerable from the sources available to the job.
    Unknown,
}

/// Calibrated confidence attached to a research claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimConfidence {
    /// Strong, directly applicable evidence.
    High,
    /// Relevant evidence with a material limitation.
    Medium,
    /// Weak, indirect, stale, or incomplete evidence.
    Low,
    /// No supported conclusion can be drawn.
    None,
}

/// Disposition of every source class required by the immutable brief.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceAudit {
    /// Source classes that were queried and returned usable evidence.
    pub searched: Vec<String>,
    /// Source classes that were reachable but returned no relevant evidence.
    pub empty: Vec<String>,
    /// Source classes that could not be reached with the granted tools.
    pub unavailable: Vec<String>,
    /// Source classes deliberately omitted with the reason in report limitations.
    pub skipped: Vec<String>,
}

impl SourceAudit {
    fn validate(&self) -> Result<BTreeSet<&str>, ResearchReportError> {
        let mut classes = BTreeSet::new();
        for class in self
            .searched
            .iter()
            .chain(&self.empty)
            .chain(&self.unavailable)
            .chain(&self.skipped)
        {
            let class = class.trim();
            if class.is_empty() {
                return Err(ResearchReportError::BlankSourceClass);
            }
            if !classes.insert(class) {
                return Err(ResearchReportError::DuplicateSourceClass(class.to_owned()));
            }
        }
        if classes.is_empty() {
            return Err(ResearchReportError::NoSourceClasses);
        }
        Ok(classes)
    }
}

/// One normalized claim and its revision-pinned evidence reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchClaim {
    /// Whether the claim is direct, inferred, contradicted, or unknown.
    pub kind: ClaimKind,
    /// Concise claim text.
    pub text: String,
    /// Source class named in [`SourceAudit`].
    pub source_type: String,
    /// Resolved commit, document version, or other immutable revision.
    pub revision: String,
    /// Permalink or repository path with a tight symbol/line reference.
    pub location: String,
    /// Observation timestamp supplied by the worker.
    pub observed_at: String,
    /// Confidence calibrated to the cited evidence.
    pub confidence: ClaimConfidence,
    /// Explicit caveats; use `"none"` only when there is genuinely no caveat.
    pub limitations: Vec<String>,
}

impl ResearchClaim {
    fn validate(&self, audit: &SourceAudit) -> Result<(), ResearchReportError> {
        for (field, value) in [
            ("text", self.text.as_str()),
            ("source_type", self.source_type.as_str()),
            ("revision", self.revision.as_str()),
            ("location", self.location.as_str()),
            ("observed_at", self.observed_at.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ResearchReportError::BlankClaimField(field));
            }
        }
        if !is_rfc3339(&self.observed_at) {
            return Err(ResearchReportError::InvalidObservationTime);
        }
        if !is_immutable_revision(&self.revision) {
            return Err(ResearchReportError::MutableRevision);
        }
        if !is_tight_location(&self.location) {
            return Err(ResearchReportError::ImpreciseLocation);
        }
        if self.limitations.is_empty()
            || self
                .limitations
                .iter()
                .any(|limitation| limitation.trim().is_empty())
        {
            return Err(ResearchReportError::InvalidLimitations);
        }
        let source_type = self.source_type.trim();
        if !audit
            .searched
            .iter()
            .chain(&audit.empty)
            .chain(&audit.unavailable)
            .chain(&audit.skipped)
            .any(|class| class.trim() == source_type)
        {
            return Err(ResearchReportError::UnauditedSourceClass(
                source_type.to_owned(),
            ));
        }
        match self.kind {
            ClaimKind::Direct | ClaimKind::Inferred | ClaimKind::Contradicted
                if !audit
                    .searched
                    .iter()
                    .any(|class| class.trim() == source_type) =>
            {
                Err(ResearchReportError::UnsupportedClaimSource(
                    source_type.to_owned(),
                ))
            }
            ClaimKind::Unknown if self.confidence != ClaimConfidence::None => {
                Err(ResearchReportError::UnknownClaimHasConfidence)
            }
            ClaimKind::Direct if self.confidence == ClaimConfidence::None => {
                Err(ResearchReportError::DirectClaimHasNoConfidence)
            }
            ClaimKind::Inferred | ClaimKind::Contradicted
                if self.confidence == ClaimConfidence::None =>
            {
                Err(ResearchReportError::SupportedClaimHasNoConfidence)
            }
            _ => Ok(()),
        }
    }
}

fn is_rfc3339(value: &str) -> bool {
    DateTime::parse_from_rfc3339(value).is_ok()
        && value.as_bytes().get(10) == Some(&b'T')
        && !value.starts_with("0000-")
}

fn is_immutable_revision(value: &str) -> bool {
    let value = value.trim();
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_tight_location(value: &str) -> bool {
    let value = value.trim();
    (value.starts_with("https://")
        && value.strip_prefix("https://").is_some_and(|rest| {
            rest.contains('/') && (rest.contains('#') || rest.contains("/blob/"))
        }))
        || (value.contains('/')
            && value.rsplit_once(':').is_some_and(|(_, anchor)| {
                !anchor.is_empty()
                    && anchor
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            }))
}

/// Reserved read-only critic result over the normalized claims.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CriticAttestation {
    /// Profile or session identity distinct from the research worker.
    pub critic_id: String,
    /// Whether normalized claims, links, and secret safety passed review.
    pub passed: bool,
    /// SHA-256 digest of the normalized claim manifest reviewed by the critic.
    pub claims_digest: String,
    /// Explicit critic findings; use `"none"` only when no finding remains.
    pub findings: Vec<String>,
}

/// Complete machine-readable output of one read-only investigation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchReport {
    /// Report schema version. The only supported value is `1`.
    pub schema_version: u32,
    /// Exact question copied from the immutable job brief.
    pub question: String,
    /// Observable condition that makes the investigation complete.
    pub done_predicate: String,
    /// Normalized evidence claims.
    pub claims: Vec<ResearchClaim>,
    /// Disposition of every source class required by the brief.
    pub sources: SourceAudit,
    /// Reserved attestation populated only after the live critic topology exists.
    pub critic: Option<CriticAttestation>,
    /// Report-level caveats and exact follow-up actions.
    pub limitations: Vec<String>,
}

impl ResearchReport {
    /// Validates the fail-closed Step 5 report contract.
    ///
    /// # Errors
    ///
    /// Returns [`ResearchReportError`] when the schema, evidence attribution,
    /// source audit, or confidence semantics are incomplete or inconsistent.
    // coverage-critical
    pub fn validate(&self) -> Result<(), ResearchReportError> {
        if self.schema_version != 1 {
            return Err(ResearchReportError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        if self.question.trim().is_empty() {
            return Err(ResearchReportError::BlankQuestion);
        }
        if self.done_predicate.trim().is_empty() {
            return Err(ResearchReportError::BlankDonePredicate);
        }
        if self.claims.is_empty() {
            return Err(ResearchReportError::NoClaims);
        }
        if self.limitations.is_empty()
            || self
                .limitations
                .iter()
                .any(|limitation| limitation.trim().is_empty())
        {
            return Err(ResearchReportError::InvalidLimitations);
        }
        if let Some(critic) = &self.critic
            && (critic.critic_id.trim().is_empty()
                || !critic.passed
                || critic.claims_digest.len() != 64
                || !critic
                    .claims_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || critic.findings.is_empty()
                || critic
                    .findings
                    .iter()
                    .any(|finding| finding.trim().is_empty()))
        {
            return Err(ResearchReportError::InvalidCriticAttestation);
        }
        self.sources.validate()?;
        for claim in &self.claims {
            claim.validate(&self.sources)?;
        }
        Ok(())
    }

    /// Validates the report and binds its question to one immutable brief.
    ///
    /// # Errors
    ///
    /// Returns [`ResearchReportError::QuestionMismatch`] when the report was
    /// produced for a different goal, in addition to the ordinary report
    /// validation failures.
    pub fn validate_for_brief(&self, brief: &JobBrief) -> Result<(), ResearchReportError> {
        self.validate()?;
        if self.question != brief.goal {
            return Err(ResearchReportError::QuestionMismatch);
        }
        Ok(())
    }
}

/// Invalid or internally inconsistent research output.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ResearchReportError {
    /// Only the current fail-closed schema is accepted.
    #[error("unsupported research report schema version: {0}")]
    UnsupportedSchemaVersion(u32),
    /// The brief question is required.
    #[error("research question must not be blank")]
    BlankQuestion,
    /// The report must state when the work is done.
    #[error("research done predicate must not be blank")]
    BlankDonePredicate,
    /// A successful report must contain at least one claim.
    #[error("research report must contain at least one claim")]
    NoClaims,
    /// At least one source class must be accounted for.
    #[error("research report must account for at least one source class")]
    NoSourceClasses,
    /// Empty source class names are ambiguous.
    #[error("research source class must not be blank")]
    BlankSourceClass,
    /// One source class may have only one disposition.
    #[error("research source class has multiple dispositions: {0}")]
    DuplicateSourceClass(String),
    /// A required claim field is blank.
    #[error("research claim field must not be blank: {0}")]
    BlankClaimField(&'static str),
    /// Observation times must be machine-comparable and include an offset.
    #[error("research claim observation time must be RFC 3339 with an explicit offset")]
    InvalidObservationTime,
    /// Evidence revisions must be immutable hexadecimal identifiers.
    #[error("research claim revision must be a full immutable digest")]
    MutableRevision,
    /// Evidence locations must be permalinks or paths with a symbol/line anchor.
    #[error("research claim location must be a permalink or tightly anchored path")]
    ImpreciseLocation,
    /// Claims and reports must state their limitations explicitly.
    #[error("research limitations must contain non-blank entries")]
    InvalidLimitations,
    /// The claim names a source class omitted from the audit.
    #[error("claim source class was not audited: {0}")]
    UnauditedSourceClass(String),
    /// A supported claim cites a source that was not actually searched.
    #[error("supported claim cites an unsearched source class: {0}")]
    UnsupportedClaimSource(String),
    /// Unknown means the evidence supports no confidence level.
    #[error("unknown claims must use confidence none")]
    UnknownClaimHasConfidence,
    /// A direct claim must carry a real confidence assessment.
    #[error("direct claims cannot use confidence none")]
    DirectClaimHasNoConfidence,
    /// Inferred and contradicted claims still require calibrated confidence.
    #[error("supported claims cannot use confidence none")]
    SupportedClaimHasNoConfidence,
    /// A report requires a successful independent critic attestation.
    #[error("research critic attestation is incomplete or unsuccessful")]
    InvalidCriticAttestation,
    /// Research output cannot be replayed under another immutable goal.
    #[error("research report question differs from brief goal")]
    QuestionMismatch,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        ClaimConfidence, ClaimKind, CriticAttestation, ResearchClaim, ResearchReport,
        ResearchReportError, SourceAudit,
    };
    use crate::{
        JobBrief, JobId, NetworkMode, NetworkPolicy, PathPolicy, ResourceLimits, RiskClass, Sha,
        UnitId, VerificationCommand,
    };

    fn brief() -> JobBrief {
        JobBrief {
            job_id: JobId::new("research-job").expect("job id"),
            unit_id: UnitId::new("research-unit").expect("unit id"),
            goal: "What changed?".to_owned(),
            repository: "https://example.invalid/nswarm.git".to_owned(),
            base_sha: Sha::new("a".repeat(40)).expect("base SHA"),
            paths: PathPolicy {
                readable: vec![
                    PathBuf::from("crates/assigned"),
                    PathBuf::from("artifacts/research"),
                ],
                writable: vec![PathBuf::from("artifacts/research")],
                forbidden: vec![PathBuf::from("secrets")],
            },
            dependencies: Vec::new(),
            acceptance_criteria: vec!["Every source class is accounted for.".to_owned()],
            verification_commands: vec![VerificationCommand {
                program: "validate-report".to_owned(),
                arguments: Vec::new(),
            }],
            risk_class: RiskClass::Low,
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
            credential_grants: Vec::new(),
            report_schema: json!({
                "type": "object",
                "required": ["schema_version"],
                "properties": {"schema_version": {"type": "integer"}},
                "additionalProperties": true
            }),
            standing_policy_version: "v1".to_owned(),
        }
    }

    fn report() -> ResearchReport {
        ResearchReport {
            schema_version: 1,
            question: "What changed?".to_owned(),
            done_predicate: "Every required source class is accounted for.".to_owned(),
            claims: vec![ResearchClaim {
                kind: ClaimKind::Direct,
                text: "The parser rejects blank input.".to_owned(),
                source_type: "source-control".to_owned(),
                revision: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                location: "crates/example/src/lib.rs:parse".to_owned(),
                observed_at: "2026-08-31T08:00:00Z".to_owned(),
                confidence: ClaimConfidence::High,
                limitations: vec!["Runtime deployment was not observed.".to_owned()],
            }],
            sources: SourceAudit {
                searched: vec!["source-control".to_owned()],
                empty: vec!["issues".to_owned()],
                unavailable: vec!["observability".to_owned()],
                skipped: vec!["analytics".to_owned()],
            },
            critic: Some(CriticAttestation {
                critic_id: "critic-research-unit".to_owned(),
                passed: true,
                claims_digest: "b".repeat(64),
                findings: vec!["none".to_owned()],
            }),
            limitations: vec!["No production access was granted.".to_owned()],
        }
    }

    #[test]
    fn complete_research_report_is_accepted() {
        assert_eq!(report().validate(), Ok(()));
        assert_eq!(report().validate_for_brief(&brief()), Ok(()));

        let mut wrong = brief();
        wrong.goal = "A different question".to_owned();
        assert_eq!(
            report().validate_for_brief(&wrong),
            Err(ResearchReportError::QuestionMismatch)
        );
    }

    #[test]
    fn source_dispositions_are_exclusive_and_complete() {
        let mut duplicate = report();
        duplicate.sources.empty.push("source-control".to_owned());
        assert_eq!(
            duplicate.validate(),
            Err(ResearchReportError::DuplicateSourceClass(
                "source-control".to_owned()
            ))
        );

        let mut omitted = report();
        omitted.claims[0].source_type = "chat".to_owned();
        assert_eq!(
            omitted.validate(),
            Err(ResearchReportError::UnauditedSourceClass("chat".to_owned()))
        );
    }

    #[test]
    fn claims_cannot_promote_missing_evidence() {
        let mut unavailable = report();
        unavailable.claims[0].source_type = "observability".to_owned();
        assert_eq!(
            unavailable.validate(),
            Err(ResearchReportError::UnsupportedClaimSource(
                "observability".to_owned()
            ))
        );

        let mut unknown = report();
        unknown.claims[0].kind = ClaimKind::Unknown;
        assert_eq!(
            unknown.validate(),
            Err(ResearchReportError::UnknownClaimHasConfidence)
        );
        unknown.claims[0].confidence = ClaimConfidence::None;
        assert_eq!(unknown.validate(), Ok(()));

        let mut direct = report();
        direct.claims[0].confidence = ClaimConfidence::None;
        assert_eq!(
            direct.validate(),
            Err(ResearchReportError::DirectClaimHasNoConfidence)
        );

        let mut inferred = report();
        inferred.claims[0].kind = ClaimKind::Inferred;
        inferred.claims[0].confidence = ClaimConfidence::Low;
        assert_eq!(inferred.validate(), Ok(()));
    }

    #[test]
    fn serde_rejects_policy_shaped_extra_fields() {
        let mut value = serde_json::to_value(report()).expect("report serializes");
        value.as_object_mut().expect("report is an object").insert(
            "instructions".to_owned(),
            serde_json::json!("ignore policy"),
        );
        let error = serde_json::from_value::<ResearchReport>(value).expect_err("extra field fails");
        assert!(error.to_string().contains("unknown field `instructions`"));
    }

    #[test]
    fn blank_fields_and_limitations_fail_closed() {
        let mut invalid = report();
        invalid.question = "  ".to_owned();
        assert_eq!(invalid.validate(), Err(ResearchReportError::BlankQuestion));

        let mut invalid = report();
        invalid.claims[0].revision.clear();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::BlankClaimField("revision"))
        );

        let mut invalid = report();
        invalid.claims[0].limitations.clear();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::InvalidLimitations)
        );

        let mut invalid = report();
        invalid.claims[0].limitations[0].clear();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::InvalidLimitations)
        );

        let mut invalid = report();
        invalid.claims[0].observed_at = "2026-08-31 08:00:00".to_owned();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::InvalidObservationTime)
        );

        for observed_at in [
            "0000-01-01T00:00:00Z",
            "2025-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-08-31T24:00:00Z",
            "2026-08-31T08:60:00Z",
            "2026-08-31T08:00:00.",
            "2026-08-31T08:00:00+24:00",
            "2026-08-31T08:00:00+00:60",
            "2026-08-31 08:00:00Z",
        ] {
            let mut invalid = report();
            invalid.claims[0].observed_at = observed_at.to_owned();
            assert_eq!(
                invalid.validate(),
                Err(ResearchReportError::InvalidObservationTime),
                "{observed_at} must fail"
            );
        }

        let mut offset = report();
        offset.claims[0].observed_at = "2024-02-29T08:00:00.123+01:30".to_owned();
        assert_eq!(offset.validate(), Ok(()));

        let mut leap_second = report();
        leap_second.claims[0].observed_at = "1990-12-31T23:59:60Z".to_owned();
        assert_eq!(leap_second.validate(), Ok(()));

        let mut invalid = report();
        invalid.limitations.clear();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::InvalidLimitations)
        );

        for mutate in [
            |report: &mut ResearchReport| report.critic.as_mut().unwrap().critic_id.clear(),
            |report: &mut ResearchReport| report.critic.as_mut().unwrap().passed = false,
            |report: &mut ResearchReport| {
                report.critic.as_mut().unwrap().claims_digest.pop();
            },
            |report: &mut ResearchReport| {
                report.critic.as_mut().unwrap().claims_digest = "g".repeat(64);
            },
            |report: &mut ResearchReport| report.critic.as_mut().unwrap().findings.clear(),
            |report: &mut ResearchReport| report.critic.as_mut().unwrap().findings[0].clear(),
        ] {
            let mut invalid = report();
            mutate(&mut invalid);
            assert_eq!(
                invalid.validate(),
                Err(ResearchReportError::InvalidCriticAttestation)
            );
        }
    }

    #[test]
    fn revisions_and_locations_have_exact_boundaries() {
        let mut invalid = report();
        invalid.claims[0].revision = "latest".to_owned();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::MutableRevision)
        );

        let mut digest_revision = report();
        digest_revision.claims[0].revision = "b".repeat(64);
        assert_eq!(digest_revision.validate(), Ok(()));

        for revision in ["b".repeat(39), "b".repeat(41), "b".repeat(63)] {
            let mut invalid = report();
            invalid.claims[0].revision = revision;
            assert_eq!(
                invalid.validate(),
                Err(ResearchReportError::MutableRevision)
            );
        }

        for location in [
            "the parser",
            "https://example.com",
            "https://example.com/main",
        ] {
            let mut invalid = report();
            invalid.claims[0].location = location.to_owned();
            assert_eq!(
                invalid.validate(),
                Err(ResearchReportError::ImpreciseLocation)
            );
        }

        for location in [
            "https://example.com/repository/blob/aaaaaaaa/file.rs",
            "https://example.com/document#section",
            "crates/example/src/lib.rs:parse_value",
            "crates/example/src/lib.rs:parse-value",
            "crates/example/src/lib.rs:10-20",
        ] {
            let mut valid = report();
            valid.claims[0].location = location.to_owned();
            assert_eq!(valid.validate(), Ok(()), "{location} must pass");
        }
    }

    #[test]
    fn report_shape_and_confidence_branches_fail_closed() {
        let mut invalid = report();
        invalid.schema_version = 2;
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::UnsupportedSchemaVersion(2))
        );

        let mut invalid = report();
        invalid.done_predicate.clear();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::BlankDonePredicate)
        );

        let mut invalid = report();
        invalid.claims.clear();
        assert_eq!(invalid.validate(), Err(ResearchReportError::NoClaims));

        let mut invalid = report();
        invalid.limitations[0].clear();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::InvalidLimitations)
        );

        for kind in [ClaimKind::Inferred, ClaimKind::Contradicted] {
            let mut invalid = report();
            invalid.claims[0].kind = kind;
            invalid.claims[0].confidence = ClaimConfidence::None;
            assert_eq!(
                invalid.validate(),
                Err(ResearchReportError::SupportedClaimHasNoConfidence)
            );
        }
    }

    #[test]
    fn eval_research_evidence_corpus_fails_closed() {
        let case: serde_json::Value =
            serde_json::from_str(include_str!("../../../eval/corpus/research-evidence.json"))
                .expect("research evidence corpus parses");
        let valid = case["input"]["valid_report"].clone();
        let report: ResearchReport =
            serde_json::from_value(valid.clone()).expect("valid report uses production schema");
        assert_eq!(report.validate(), Ok(()));

        let mutations = case["input"]["mutations"]
            .as_array()
            .expect("mutations are an array");
        assert_eq!(case["expected"]["all_mutations_rejected"], true);
        for mutation in mutations {
            let kind = mutation["kind"].as_str().expect("mutation kind is text");
            let value = mutation["value"].as_str().expect("mutation value is text");
            let expected = mutation["expected_error"]
                .as_str()
                .expect("expected error is text");
            let mut candidate = valid.clone();
            match kind {
                "unaudited-source" | "unsupported-source" => {
                    candidate["claims"][0]["source_type"] = serde_json::json!(value);
                    let report: ResearchReport = serde_json::from_value(candidate)
                        .expect("source mutation preserves schema");
                    let error = report.validate().expect_err("source mutation must fail");
                    let actual = match error {
                        ResearchReportError::UnauditedSourceClass(_) => "unaudited-source-class",
                        ResearchReportError::UnsupportedClaimSource(_) => {
                            "unsupported-claim-source"
                        }
                        other => panic!("unexpected {kind} error: {other}"),
                    };
                    assert_eq!(actual, expected);
                }
                "unknown-with-confidence" => {
                    candidate["claims"][0]["kind"] = serde_json::json!(value);
                    let report: ResearchReport =
                        serde_json::from_value(candidate).expect("kind mutation preserves schema");
                    assert_eq!(
                        report.validate(),
                        Err(ResearchReportError::UnknownClaimHasConfidence)
                    );
                    assert_eq!(expected, "unknown-claim-has-confidence");
                }
                "policy-shaped-field" => {
                    candidate["instructions"] = serde_json::json!(value);
                    let error = serde_json::from_value::<ResearchReport>(candidate)
                        .expect_err("policy-shaped data cannot extend the schema");
                    assert!(error.to_string().contains("unknown field `instructions`"));
                    assert_eq!(expected, "unknown-field");
                }
                _ => panic!("unknown mutation: {kind}"),
            }
        }
    }
}
