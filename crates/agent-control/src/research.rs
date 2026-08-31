//! Machine-validated evidence reports for the Step 5 research profile.

use std::collections::BTreeSet;

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
            _ => Ok(()),
        }
    }
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
    /// Research output cannot be replayed under another immutable goal.
    #[error("research report question differs from brief goal")]
    QuestionMismatch,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        ClaimConfidence, ClaimKind, ResearchClaim, ResearchReport, ResearchReportError, SourceAudit,
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
        invalid.limitations.clear();
        assert_eq!(
            invalid.validate(),
            Err(ResearchReportError::InvalidLimitations)
        );
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
        assert_eq!(
            mutations.len() as u64,
            case["expected"]["mutation_count"]
                .as_u64()
                .expect("expected mutation count")
        );
        for mutation in mutations {
            let kind = mutation["kind"].as_str().expect("mutation kind is text");
            let value = mutation["value"].as_str().expect("mutation value is text");
            let mut candidate = valid.clone();
            match kind {
                "unaudited-source" | "unsupported-source" => {
                    candidate["claims"][0]["source_type"] = serde_json::json!(value);
                    let report: ResearchReport = serde_json::from_value(candidate)
                        .expect("source mutation preserves schema");
                    assert!(report.validate().is_err(), "{kind} must fail");
                }
                "unknown-with-confidence" => {
                    candidate["claims"][0]["kind"] = serde_json::json!(value);
                    let report: ResearchReport =
                        serde_json::from_value(candidate).expect("kind mutation preserves schema");
                    assert_eq!(
                        report.validate(),
                        Err(ResearchReportError::UnknownClaimHasConfidence)
                    );
                }
                "policy-shaped-field" => {
                    candidate["instructions"] = serde_json::json!(value);
                    let error = serde_json::from_value::<ResearchReport>(candidate)
                        .expect_err("policy-shaped data cannot extend the schema");
                    assert!(error.to_string().contains("unknown field `instructions`"));
                }
                _ => panic!("unknown mutation: {kind}"),
            }
        }
    }
}
