use std::path::Path;

use crate::error::{Error, Result};
use crate::model::{new_id, now_rfc3339, Evidence};
use crate::repo::Repo;

/// Where a piece of evidence came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSource {
    /// A command KAP ran and observed.
    ToolExecution,
    /// A change to the files, observed by KAP.
    FileChange,
    /// Test, benchmark, or check output from a real run.
    TestResult,
    /// Something an agent said about its own work. A claim, not an observation.
    AgentReport,
    /// A file the agent produced or found — logs, plots, datasets, reports.
    ExternalArtifact,
}

impl EvidenceSource {
    /// The value stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceSource::ToolExecution => "tool_execution",
            EvidenceSource::FileChange => "file_change",
            EvidenceSource::TestResult => "test_result",
            EvidenceSource::AgentReport => "agent_report",
            EvidenceSource::ExternalArtifact => "external_artifact",
        }
    }

    /// Parse a source from its database value.
    pub fn parse(value: &str) -> Option<EvidenceSource> {
        match value {
            "tool_execution" => Some(EvidenceSource::ToolExecution),
            "file_change" => Some(EvidenceSource::FileChange),
            "test_result" => Some(EvidenceSource::TestResult),
            "agent_report" => Some(EvidenceSource::AgentReport),
            "external_artifact" => Some(EvidenceSource::ExternalArtifact),
            _ => None,
        }
    }

    /// Whether this source describes something KAP observed itself.
    fn is_observation(self) -> bool {
        !matches!(self, EvidenceSource::AgentReport)
    }
}

/// Parameters for [`Repo::attach_evidence`].
#[derive(Debug, Clone, Copy)]
pub struct NewEvidence<'a> {
    /// Where the evidence comes from.
    pub source: EvidenceSource,
    /// The command the evidence came from, when there is one.
    pub command: Option<&'a str>,
    /// The command's exit code.
    pub exit_code: Option<i32>,
    /// A file to store as the artifact behind this evidence.
    pub artifact: Option<&'a Path>,
    /// The decision this evidence supports or undermines.
    pub decision: Option<&'a str>,
    /// Whether the agent or the user considers the evidence whole.
    pub complete: bool,
}

impl Repo {
    /// Record evidence for an attempt.
    ///
    /// Evidence is never taken on trust: an artifact is stored and hashed, so the bytes behind a
    /// record can be checked later. A report with no artifact is recorded as unobserved and
    /// incomplete — it is a claim, and the database says so.
    pub fn attach_evidence(&self, attempt_id: &str, new: NewEvidence<'_>) -> Result<Evidence> {
        let db = self.db()?;
        db.attempt(attempt_id)?;

        if let Some(decision_id) = new.decision {
            let belongs_to_attempt = db
                .decisions(attempt_id)?
                .iter()
                .any(|decision| decision.id == decision_id);
            if !belongs_to_attempt {
                return Err(Error::DecisionNotFound {
                    id: decision_id.to_string(),
                    attempt: attempt_id.to_string(),
                });
            }
        }

        let artifact_hash = match new.artifact {
            Some(path) => {
                let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
                Some(self.objects().put(&bytes)?)
            }
            None => None,
        };

        let observed = new.source.is_observation() && artifact_hash.is_some();
        let evidence = Evidence {
            id: format!("evidence-{}", new_id()),
            attempt_id: attempt_id.to_string(),
            decision_id: new.decision.map(str::to_string),
            source: new.source.as_str().to_string(),
            command: new.command.map(str::to_string),
            exit_code: new.exit_code,
            artifact_hash,
            path: None,
            observed,
            complete: new.complete && observed,
            recorded_at: now_rfc3339(),
        };

        self.db()?.insert_evidence(&evidence)?;
        Ok(evidence)
    }

    /// Evidence recorded for an attempt, oldest first.
    pub fn evidence(&self, attempt_id: &str) -> Result<Vec<Evidence>> {
        self.db()?.evidence(attempt_id)
    }
}
