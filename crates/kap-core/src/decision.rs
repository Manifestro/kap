use crate::error::Result;
use crate::model::{new_id, now_rfc3339, Decision};
use crate::repo::Repo;

/// Parameters for [`Repo::add_decision`].
#[derive(Debug, Clone, Copy)]
pub struct NewDecision<'a> {
    /// What was decided.
    pub summary: &'a str,
    /// Why — the reasoning the agent stands behind.
    pub rationale: Option<&'a str>,
    /// What else was considered and why it lost.
    pub alternatives: Option<&'a str>,
}

impl Repo {
    /// Record a decision made inside an attempt.
    ///
    /// A decision is a declaration, not an observation: it is stored with source `agent_report`
    /// so it is never mistaken for evidence.
    pub fn add_decision(&self, attempt_id: &str, new: NewDecision<'_>) -> Result<Decision> {
        // Fail before writing if the attempt does not exist.
        self.db()?.attempt(attempt_id)?;

        let decision = Decision {
            id: format!("decision-{}", new_id()),
            attempt_id: attempt_id.to_string(),
            summary: new.summary.to_string(),
            rationale: new.rationale.map(str::to_string),
            alternatives: new.alternatives.map(str::to_string),
            source: "agent_report".to_string(),
            created_at: now_rfc3339(),
        };

        self.db()?.insert_decision(&decision)?;
        Ok(decision)
    }

    /// Decisions recorded for an attempt, oldest first.
    pub fn decisions(&self, attempt_id: &str) -> Result<Vec<Decision>> {
        self.db()?.decisions(attempt_id)
    }
}
