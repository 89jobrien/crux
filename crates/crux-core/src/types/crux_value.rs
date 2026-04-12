/// The core `Crux<T>` type — a value fused with its causal execution trace.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::error::CruxErr;
use super::id::CruxId;
use super::step::{Step, StepKind, StepStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crux<T> {
    pub id: CruxId,
    pub agent: String,
    pub value: Result<T, CruxErr>,
    pub steps: Vec<Step>,
    pub children: Vec<Crux<serde_json::Value>>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Info about a delegation extracted from the trace.
#[derive(Debug)]
pub struct Delegation<'a> {
    pub from_agent: &'a str,
    pub to_agent: &'a str,
    pub child: &'a Crux<serde_json::Value>,
}

impl<T> Crux<T> {
    /// Extract the inner result.
    pub fn value(&self) -> Result<&T, &CruxErr> {
        self.value.as_ref()
    }

    /// Consume and extract the inner result.
    pub fn into_value(self) -> Result<T, CruxErr> {
        self.value
    }

    /// All steps in causal order (flat, this agent only).
    pub fn causal_chain(&self) -> Vec<&Step> {
        self.steps.iter().collect()
    }

    /// All delegation steps with their child crux.
    pub fn delegations(&self) -> Vec<Delegation<'_>> {
        let delegation_steps: Vec<_> = self
            .steps
            .iter()
            .filter(|s| s.kind == StepKind::Delegation)
            .collect();

        delegation_steps
            .into_iter()
            .zip(self.children.iter())
            .map(|(_step, child)| Delegation {
                from_agent: &self.agent,
                to_agent: &child.agent,
                child,
            })
            .collect()
    }

    /// Steps that were considered but rejected (e.g., speculation losers).
    pub fn rejected_branches(&self) -> Vec<&Step> {
        self.steps
            .iter()
            .filter(|s| s.status == StepStatus::Rejected)
            .collect()
    }

    /// Total duration in milliseconds.
    pub fn duration_ms(&self) -> Option<u64> {
        self.finished_at
            .map(|end| (end - self.started_at).num_milliseconds().unsigned_abs())
    }

    /// Number of steps that completed successfully.
    pub fn succeeded_count(&self) -> usize {
        self.steps.iter().filter(|s| s.is_ok()).count()
    }

    /// Number of steps that failed.
    pub fn failed_count(&self) -> usize {
        self.steps.iter().filter(|s| s.is_err()).count()
    }
}

impl<T: Serialize> Crux<T> {
    /// Snapshot the current state as a type-erased Crux for checkpointing.
    pub fn to_snapshot(&self) -> Result<Crux<serde_json::Value>, serde_json::Error> {
        let value = match &self.value {
            Ok(v) => Ok(serde_json::to_value(v)?),
            Err(e) => {
                let err_json = serde_json::to_string(e)?;
                Err(serde_json::from_str(&err_json)?)
            }
        };
        Ok(Crux {
            id: self.id.clone(),
            agent: self.agent.clone(),
            value,
            steps: self.steps.clone(),
            children: self.children.clone(),
            started_at: self.started_at,
            finished_at: self.finished_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_crux() -> Crux<String> {
        Crux {
            id: CruxId::new(),
            agent: "test".into(),
            value: Ok("hello".into()),
            steps: vec![
                Step {
                    name: "greet".into(),
                    kind: StepKind::Plain,
                    status: StepStatus::Ok,
                    confidence: 1.0,
                    started_at: Utc::now(),
                    duration_ms: 5,
                    input_hash: 0,
                    content_hash: None,
                    output: Some(serde_json::json!("hello")),
                    error: None,
                    attempt: 1,
                    events: vec![],
                },
                Step {
                    name: "rejected_branch".into(),
                    kind: StepKind::Speculation,
                    status: StepStatus::Rejected,
                    confidence: 0.3,
                    started_at: Utc::now(),
                    duration_ms: 2,
                    input_hash: 0,
                    content_hash: None,
                    output: None,
                    error: None,
                    attempt: 1,
                    events: vec![],
                },
            ],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    #[test]
    fn value_extraction() {
        let crux = sample_crux();
        assert_eq!(crux.value().unwrap(), "hello");
    }

    #[test]
    fn causal_chain_returns_all_steps() {
        let crux = sample_crux();
        assert_eq!(crux.causal_chain().len(), 2);
    }

    #[test]
    fn rejected_branches_filters() {
        let crux = sample_crux();
        let rejected = crux.rejected_branches();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].name, "rejected_branch");
    }

    #[test]
    fn succeeded_and_failed_counts() {
        let crux = sample_crux();
        assert_eq!(crux.succeeded_count(), 1);
        assert_eq!(crux.failed_count(), 0);
    }

    #[test]
    fn serde_round_trip() {
        let crux = sample_crux();
        let json = serde_json::to_string_pretty(&crux).unwrap();
        let back: Crux<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.value().unwrap(), "hello");
        assert_eq!(back.steps.len(), 2);
    }
}
