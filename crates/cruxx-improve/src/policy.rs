use crate::improvement::StrategyDiff;
use thiserror::Error;

/// Why a strategy policy rejected a proposed diff.
#[derive(Debug, Clone, Error)]
pub enum StrategyViolation {
    #[error("too many simultaneous changes: {count} (max {max})")]
    TooManyChanges { count: usize, max: usize },
    #[error("strategy violation: {reason}")]
    Custom { reason: String },
}

/// Port: validates proposed strategy changes.
///
/// Extends the SafetyPolicy concept from harness diffs to strategy diffs.
pub trait StrategyPolicy: Send + Sync {
    fn validate_strategy(&self, diff: &StrategyDiff) -> Result<(), StrategyViolation>;
    fn requires_strategy_approval(&self, diff: &StrategyDiff) -> bool;
}

/// Default policy: caps simultaneous changes, requires approval for
/// prompt patches and delegation rules (high-risk), auto-approves
/// threshold tweaks and tool preferences (low-risk).
#[derive(Debug, Clone)]
pub struct DefaultStrategyPolicy {
    pub max_simultaneous_changes: usize,
}

impl Default for DefaultStrategyPolicy {
    fn default() -> Self {
        Self {
            max_simultaneous_changes: 10,
        }
    }
}

impl StrategyPolicy for DefaultStrategyPolicy {
    fn validate_strategy(&self, diff: &StrategyDiff) -> Result<(), StrategyViolation> {
        let count = diff.tool_preferences.len()
            + diff.confidence_thresholds.len()
            + diff.delegation_rules.len()
            + diff.prompt_patches.len();
        if count > self.max_simultaneous_changes {
            return Err(StrategyViolation::TooManyChanges {
                count,
                max: self.max_simultaneous_changes,
            });
        }
        Ok(())
    }

    fn requires_strategy_approval(&self, diff: &StrategyDiff) -> bool {
        !diff.prompt_patches.is_empty() || !diff.delegation_rules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::improvement::{PromptPatch, StrategyDiff};

    #[test]
    fn default_policy_allows_small_changes() {
        let policy = DefaultStrategyPolicy::default();
        let diff = StrategyDiff {
            tool_preferences: vec![("rg".into(), 5)],
            ..Default::default()
        };
        assert!(policy.validate_strategy(&diff).is_ok());
    }

    #[test]
    fn default_policy_rejects_too_many_changes() {
        let policy = DefaultStrategyPolicy {
            max_simultaneous_changes: 2,
        };
        let diff = StrategyDiff {
            tool_preferences: vec![("a".into(), 1), ("b".into(), 2), ("c".into(), 3)],
            ..Default::default()
        };
        assert!(policy.validate_strategy(&diff).is_err());
    }

    #[test]
    fn requires_approval_for_prompt_patches() {
        let policy = DefaultStrategyPolicy::default();
        let diff = StrategyDiff {
            prompt_patches: vec![PromptPatch {
                agent: "a".into(),
                section: "system".into(),
                content: "new prompt".into(),
            }],
            ..Default::default()
        };
        assert!(policy.requires_strategy_approval(&diff));
    }

    #[test]
    fn does_not_require_approval_for_thresholds() {
        let policy = DefaultStrategyPolicy::default();
        let diff = StrategyDiff {
            confidence_thresholds: vec![("spec".into(), 0.7)],
            ..Default::default()
        };
        assert!(!policy.requires_strategy_approval(&diff));
    }
}
