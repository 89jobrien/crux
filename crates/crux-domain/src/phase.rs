//! Explicit execution lifecycle boundaries.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPhase {
    Parse,
    Validate,
    Planning,
    Policy,
    Execution,
    Recording,
    ReplayExport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseTransition {
    pub from: ExecutionPhase,
    pub to: ExecutionPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPhaseTransition {
    pub from: ExecutionPhase,
    pub to: ExecutionPhase,
}

impl std::fmt::Display for InvalidPhaseTransition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid execution phase transition from {:?} to {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for InvalidPhaseTransition {}

impl ExecutionPhase {
    pub fn advance(self, to: Self) -> Result<PhaseTransition, InvalidPhaseTransition> {
        let valid = matches!(
            (self, to),
            (Self::Parse, Self::Validate)
                | (Self::Validate, Self::Planning)
                | (Self::Planning, Self::Policy)
                | (Self::Policy, Self::Execution)
                | (Self::Execution, Self::Recording)
                | (Self::Recording, Self::ReplayExport)
        );
        if valid {
            Ok(PhaseTransition { from: self, to })
        } else {
            Err(InvalidPhaseTransition { from: self, to })
        }
    }
}
