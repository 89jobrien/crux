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
    UnreportedCost {
        step: String,
        source: Option<Box<CruxErr>>,
    },
    StepBudgetExceeded {
        limit: u64,
        attempted: u64,
    },
    UsdBudgetExceeded {
        limit_micros: u64,
        actual_micros: u64,
        source: Option<Box<CruxErr>>,
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
            | Self::ReplayMismatch { step, .. }
            | Self::UnreportedCost { step, .. } => Some(step),
            Self::Delegation { source, .. } => source.failed_step(),
            Self::UsdBudgetExceeded {
                source: Some(source),
                ..
            } => source.failed_step(),
            Self::BudgetExceeded { .. }
            | Self::StepBudgetExceeded { .. }
            | Self::UsdBudgetExceeded { source: None, .. }
            | Self::Cancelled { .. }
            | Self::Denied { .. } => None,
        }
    }

    /// Whether this error is likely transient and retryable.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::StepFailed { .. } => true,
            Self::BudgetExceeded { .. } => false,
            Self::UnreportedCost { .. }
            | Self::StepBudgetExceeded { .. }
            | Self::UsdBudgetExceeded { .. } => false,
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
            Self::UnreportedCost { step, .. } => {
                write!(f, "handler '{step}' did not report USD cost")
            }
            Self::StepBudgetExceeded { limit, attempted } => write!(
                f,
                "step budget exceeded: limit={limit}, attempted={attempted}"
            ),
            Self::UsdBudgetExceeded {
                limit_micros,
                actual_micros,
                ..
            } => write!(
                f,
                "USD budget exceeded: limit=${}.{:06}, used=${}.{:06}",
                limit_micros / 1_000_000,
                limit_micros % 1_000_000,
                actual_micros / 1_000_000,
                actual_micros % 1_000_000
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

impl std::error::Error for CruxErr {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Delegation { source, .. }
            | Self::UnreportedCost {
                source: Some(source),
                ..
            }
            | Self::UsdBudgetExceeded {
                source: Some(source),
                ..
            } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[cfg(feature = "miette")]
impl miette::Diagnostic for CruxErr {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let code = match self {
            Self::StepFailed { .. } => "crux::step_failed",
            Self::LowConfidence { .. } => "crux::low_confidence",
            Self::BudgetExceeded { .. } => "crux::budget_exceeded",
            Self::UnreportedCost { .. } => "crux::unreported_cost",
            Self::StepBudgetExceeded { .. } => "crux::step_budget_exceeded",
            Self::UsdBudgetExceeded { .. } => "crux::usd_budget_exceeded",
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
            Self::BudgetExceeded { .. } => Some("increase the budget or reduce usage"),
            Self::UnreportedCost { .. } => {
                Some("register the handler as metered or explicitly free before using a USD budget")
            }
            Self::StepBudgetExceeded { .. } => {
                Some("increase budget.steps or reduce handler attempts")
            }
            Self::UsdBudgetExceeded { .. } => Some("increase budget.usd or reduce handler cost"),
            Self::ReplayMismatch { .. } => Some("clear the replay cache or use lenient mode"),
            Self::LowConfidence { .. } => Some("lower the threshold or improve the step output"),
            Self::Denied { .. } => Some("check planner policy or use PassthroughPlanner"),
            _ => None,
        };
        help.map(|h| Box::new(h) as Box<dyn std::fmt::Display>)
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn miette::Diagnostic> + 'a>> {
        match self {
            Self::Delegation { source, .. }
            | Self::UnreportedCost {
                source: Some(source),
                ..
            }
            | Self::UsdBudgetExceeded {
                source: Some(source),
                ..
            } => Some(Box::new(std::iter::once(
                source.as_ref() as &dyn miette::Diagnostic
            ))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

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

    #[test]
    fn accounting_errors_round_trip_with_sources() {
        let errors = [
            CruxErr::UnreportedCost {
                step: "legacy".into(),
                source: Some(Box::new(CruxErr::step_failed("legacy", "failed"))),
            },
            CruxErr::StepBudgetExceeded {
                limit: 2,
                attempted: 3,
            },
            CruxErr::UsdBudgetExceeded {
                limit_micros: 10,
                actual_micros: 11,
                source: Some(Box::new(CruxErr::step_failed("paid", "failed"))),
            },
        ];

        for error in errors {
            let json = serde_json::to_string(&error).unwrap();
            let decoded: CruxErr = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_value(&decoded).unwrap(),
                serde_json::to_value(&error).unwrap()
            );
            assert!(!decoded.is_transient());
        }

        let unreported = CruxErr::UnreportedCost {
            step: "legacy".into(),
            source: Some(Box::new(CruxErr::step_failed("legacy", "failed"))),
        };
        assert_eq!(unreported.failed_step(), Some("legacy"));
        assert_eq!(
            unreported.source().unwrap().to_string(),
            "step 'legacy' failed: failed"
        );

        let usd = CruxErr::UsdBudgetExceeded {
            limit_micros: 10,
            actual_micros: 11,
            source: Some(Box::new(CruxErr::step_failed("paid", "failed"))),
        };
        assert_eq!(usd.failed_step(), Some("paid"));
        assert_eq!(
            usd.source().unwrap().to_string(),
            "step 'paid' failed: failed"
        );
    }

    #[cfg(feature = "miette")]
    #[test]
    fn accounting_errors_expose_miette_code_help_and_related_source() {
        use miette::Diagnostic as _;

        let errors = [
            CruxErr::UnreportedCost {
                step: "legacy".into(),
                source: Some(Box::new(CruxErr::step_failed("legacy", "failed"))),
            },
            CruxErr::StepBudgetExceeded {
                limit: 2,
                attempted: 3,
            },
            CruxErr::UsdBudgetExceeded {
                limit_micros: 10,
                actual_micros: 11,
                source: Some(Box::new(CruxErr::step_failed("paid", "failed"))),
            },
        ];
        let expected_codes = [
            "crux::unreported_cost",
            "crux::step_budget_exceeded",
            "crux::usd_budget_exceeded",
        ];

        for (error, expected_code) in errors.iter().zip(expected_codes) {
            assert_eq!(error.code().unwrap().to_string(), expected_code);
            assert!(error.help().is_some());
        }
        assert_eq!(errors[0].related().unwrap().count(), 1);
        assert!(errors[1].related().is_none());
        assert_eq!(errors[2].related().unwrap().count(), 1);
    }
}
