use serde::{Deserialize, Serialize};

/// The result of an evolution cycle — did the candidate get promoted?
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum EvolutionOutcome {
    /// Candidate beat baseline — new profile is now active.
    Promoted {
        profile_id: String,
        improvement_pct: f64,
    },
    /// Candidate did not beat baseline — discarded.
    Discarded { reason: String },
    /// Safety policy blocked the proposed diff.
    Blocked { violation: String },
    /// Approval gate rejected the escalation.
    Denied { request_summary: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_serde_round_trip() {
        let outcome = EvolutionOutcome::Promoted {
            profile_id: "evolved-v2".into(),
            improvement_pct: 15.3,
        };
        let json = serde_json::to_string(&outcome).unwrap();
        let back: EvolutionOutcome = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, EvolutionOutcome::Promoted { .. }));
    }

    #[test]
    fn discarded_carries_reason() {
        let outcome = EvolutionOutcome::Discarded {
            reason: "candidate 12% slower than baseline".into(),
        };
        if let EvolutionOutcome::Discarded { reason } = outcome {
            assert!(reason.contains("slower"));
        } else {
            panic!("expected Discarded");
        }
    }

    #[test]
    fn blocked_carries_violation() {
        let outcome = EvolutionOutcome::Blocked {
            violation: "memory exceeds hard cap (4096 MB)".into(),
        };
        assert!(matches!(outcome, EvolutionOutcome::Blocked { .. }));
    }
}
