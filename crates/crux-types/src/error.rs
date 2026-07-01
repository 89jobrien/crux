/// Domain error types for crux execution.
use serde::{Deserialize, Serialize};

use crate::budget::BudgetKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CruxErr {
    StepFailed {
        step: String,
        #[serde(rename = "message")]
        source_msg: String,
    },
    LowConfidence {
        step: String,
        score: f32,
        threshold: f32,
    },
    BudgetExceeded {
        budget_kind: BudgetKind,
        limit: u64,
        actual: u64,
    },
    Delegation {
        to: String,
        source: Box<CruxErr>,
    },
    Cancelled {
        reason: String,
    },
    ReplayMismatch {
        step: String,
        expected: u64,
        actual: u64,
    },
    /// A planner denied this step.
    Denied {
        step: String,
        reason: String,
    },
}

impl CruxErr {
    pub fn step_failed(name: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::StepFailed {
            step: name.into(),
            source_msg: msg.into(),
        }
    }

    pub fn low_confidence(name: impl Into<String>, score: f32, threshold: f32) -> Self {
        Self::LowConfidence {
            step: name.into(),
            score,
            threshold,
        }
    }

    /// Returns the name of the step that failed, if applicable.
    pub fn failed_step(&self) -> Option<&str> {
        match self {
            Self::StepFailed { step, .. }
            | Self::LowConfidence { step, .. }
            | Self::ReplayMismatch { step, .. } => Some(step),
            Self::Delegation { source, .. } => source.failed_step(),
            Self::BudgetExceeded { .. } | Self::Cancelled { .. } | Self::Denied { .. } => None,
        }
    }

    /// Whether this error is likely transient and retryable.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::StepFailed { .. } => true,
            Self::BudgetExceeded { .. } => false,
            Self::LowConfidence { .. } => true,
            Self::Delegation { source, .. } => source.is_transient(),
            Self::Cancelled { .. } => false,
            Self::ReplayMismatch { .. } => false,
            Self::Denied { .. } => false,
        }
    }
}

impl std::fmt::Display for CruxErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StepFailed { step, source_msg } => {
                write!(f, "step '{step}' failed: {source_msg}")
            }
            Self::LowConfidence {
                step,
                score,
                threshold,
            } => write!(
                f,
                "step '{step}' confidence {score:.2} below threshold {threshold:.2}"
            ),
            Self::BudgetExceeded {
                budget_kind,
                limit,
                actual,
            } => write!(
                f,
                "budget exceeded: {budget_kind:?} limit={limit}, used={actual}"
            ),
            Self::Delegation { to, source } => {
                write!(f, "delegation to '{to}' failed: {source}")
            }
            Self::Cancelled { reason } => write!(f, "cancelled: {reason}"),
            Self::ReplayMismatch {
                step,
                expected,
                actual,
            } => write!(
                f,
                "replay mismatch at '{step}': expected hash {expected}, got {actual}"
            ),
            Self::Denied { step, reason } => {
                write!(f, "step '{step}' denied by planner: {reason}")
            }
        }
    }
}

impl std::error::Error for CruxErr {}

#[cfg(feature = "miette")]
impl miette::Diagnostic for CruxErr {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let code = match self {
            Self::StepFailed { .. } => "crux::step_failed",
            Self::LowConfidence { .. } => "crux::low_confidence",
            Self::BudgetExceeded { .. } => "crux::budget_exceeded",
            Self::Delegation { .. } => "crux::delegation",
            Self::Cancelled { .. } => "crux::cancelled",
            Self::ReplayMismatch { .. } => "crux::replay_mismatch",
            Self::Denied { .. } => "crux::denied",
        };
        Some(Box::new(code))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let help: Option<&str> = match self {
            Self::BudgetExceeded { .. } => Some("increase the budget or reduce step count"),
            Self::ReplayMismatch { .. } => Some("clear the replay cache or use lenient mode"),
            Self::LowConfidence { .. } => Some("lower the threshold or improve the step output"),
            Self::Denied { .. } => Some("check planner policy or use PassthroughPlanner"),
            _ => None,
        };
        help.map(|h| Box::new(h) as Box<dyn std::fmt::Display>)
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn miette::Diagnostic> + 'a>> {
        match self {
            Self::Delegation { source, .. } => Some(Box::new(std::iter::once(
                source.as_ref() as &dyn miette::Diagnostic
            ))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_step_traverses_delegation() {
        let inner = CruxErr::step_failed("parse", "bad json");
        let outer = CruxErr::Delegation {
            to: "parser".into(),
            source: Box::new(inner),
        };
        assert_eq!(outer.failed_step(), Some("parse"));
    }

    #[test]
    fn transient_classification() {
        assert!(CruxErr::step_failed("x", "timeout").is_transient());
        assert!(
            !CruxErr::Cancelled {
                reason: "user".into()
            }
            .is_transient()
        );
    }

    #[test]
    fn serde_round_trip() {
        let err = CruxErr::step_failed("fetch", "network error");
        let json = serde_json::to_string(&err).unwrap();
        let back: CruxErr = serde_json::from_str(&json).unwrap();
        assert_eq!(back.failed_step(), Some("fetch"));
    }
}
