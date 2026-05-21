/// The core `Crux<T>` type — a value fused with its causal execution trace.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CruxErr;
use crate::id::CruxId;
use crate::step::{Step, StepKind, StepStatus};

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
    /// Produce a presentation-format JSON trace.
    ///
    /// Differs from raw serde: flattens Result, omits hashes, includes
    /// computed fields (total duration). Recursive for children.
    pub fn to_trace_json(&self) -> serde_json::Value {
        let status = match &self.value {
            Ok(_) => "ok",
            Err(_) => "error",
        };
        let steps: Vec<serde_json::Value> = self
            .steps
            .iter()
            .map(|s| {
                let mut obj = serde_json::json!({
                    "name": s.name,
                    "kind": s.kind,
                    "status": s.status,
                    "duration_ms": s.duration_ms,
                    "confidence": s.confidence,
                });
                if let Some(ref err) = s.error {
                    obj["error"] = serde_json::Value::String(err.clone());
                }
                if !s.findings.is_empty() {
                    obj["findings"] = serde_json::to_value(&s.findings).unwrap_or_default();
                }
                obj
            })
            .collect();
        let children: Vec<serde_json::Value> =
            self.children.iter().map(|c| c.to_trace_json()).collect();
        let mut trace = serde_json::json!({
            "agent": self.agent,
            "id": self.id.to_string(),
            "status": status,
            "steps": steps,
        });
        if let Some(ms) = self.duration_ms() {
            trace["duration_ms"] = serde_json::json!(ms);
        }
        if !children.is_empty() {
            trace["children"] = serde_json::json!(children);
        }
        trace
    }

    /// Render the execution trace as a Mermaid flowchart.
    ///
    /// Color coding: ok=green, err=red, rejected=gray, skipped=dashed.
    /// Delegation edges annotate the child agent name.
    pub fn to_mermaid(&self) -> String {
        let mut lines = vec!["graph TD".to_string()];
        let mut child_iter = self.children.iter();

        for (i, step) in self.steps.iter().enumerate() {
            let id = format!("s{i}");
            let label = format!("{} {}ms", step.name, step.duration_ms);
            lines.push(format!("    {id}[\"{label}\"]"));

            if i > 0 {
                let prev = format!("s{}", i - 1);
                if step.kind == StepKind::Delegation {
                    if let Some(child) = child_iter.next() {
                        lines.push(format!(
                            "    {prev} -->|\"delegate: {}\"| {id}",
                            child.agent
                        ));
                    } else {
                        lines.push(format!("    {prev} --> {id}"));
                    }
                } else {
                    let edge_label = match step.status {
                        StepStatus::Ok => "ok",
                        StepStatus::Err => "err",
                        StepStatus::Rejected => "rejected",
                        StepStatus::Skipped => "skipped",
                    };
                    lines.push(format!("    {prev} -->|\"{edge_label}\"| {id}"));
                }
            }

            let style = match step.status {
                StepStatus::Ok => "fill:#90EE90",
                StepStatus::Err => "fill:#FF6B6B",
                StepStatus::Rejected => "fill:#D3D3D3",
                StepStatus::Skipped => "fill:#FFFFFF,stroke-dasharray: 5 5",
            };
            lines.push(format!("    style {id} {style}"));
        }

        lines.join("\n")
    }

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

/// Severity-ordered outcome of a workflow execution.
/// Variants declared in ascending severity order so Ord derivation is correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FinalPhase {
    Succeeded,
    Skipped,
    Aborted,
    Failed,
    Errored, // highest severity
}

/// Per-step data used in final phase aggregation.
#[derive(Debug, Clone)]
pub struct StepRecord {
    pub alias: String,
    pub phase: FinalPhase,
    pub continue_on_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{crux_ok, step_ok};

    fn sample_crux() -> Crux<String> {
        let rejected = Step {
            name: "rejected_branch".into(),
            kind: StepKind::Speculation,
            status: StepStatus::Rejected,
            confidence: 0.3,
            ..step_ok("rejected_branch", 0, None)
        };
        crux_ok(
            "test",
            "hello".into(),
            vec![
                step_ok("greet", 0, Some(serde_json::json!("hello"))),
                rejected,
            ],
        )
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

    #[test]
    fn to_trace_json_produces_presentation_format() {
        let crux = sample_crux();
        let trace = crux.to_trace_json();
        assert_eq!(trace["agent"], "test");
        assert_eq!(trace["status"], "ok");
        assert!(trace["steps"].is_array());
        let steps = trace["steps"].as_array().unwrap();
        assert_eq!(steps[0]["name"], "greet");
        assert_eq!(steps[0]["status"], "ok");
        assert!(steps[0].get("input_hash").is_none(), "hashes omitted");
    }

    #[test]
    fn to_mermaid_produces_valid_flowchart() {
        let crux = sample_crux();
        let mermaid = crux.to_mermaid();
        assert!(mermaid.starts_with("graph TD"));
        assert!(mermaid.contains("greet"));
        assert!(mermaid.contains("fill:#90EE90"), "ok steps should be green");
        assert!(
            mermaid.contains("fill:#D3D3D3"),
            "rejected steps should be gray"
        );
    }

    #[test]
    fn step_with_findings_roundtrips() {
        use crate::step::CitedFinding;
        let mut step = step_ok("analyze", 0, None);
        step.findings.push(CitedFinding {
            message: "unused import".into(),
            source: Some("src/lib.rs::main:42".into()),
        });
        let json = serde_json::to_string(&step).unwrap();
        assert!(json.contains("unused import"));
        let back: crate::step::Step = serde_json::from_str(&json).unwrap();
        assert_eq!(back.findings.len(), 1);
        assert_eq!(
            back.findings[0].source.as_deref(),
            Some("src/lib.rs::main:42")
        );
    }

    #[test]
    fn step_without_findings_omits_field() {
        let step = step_ok("plain", 0, None);
        let json = serde_json::to_string(&step).unwrap();
        assert!(!json.contains("findings"));
    }
}
