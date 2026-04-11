/// Budget constraints for agent execution.
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Budget {
    Tokens { limit: u64 },
    Calls { limit: u64 },
    Duration { limit_ms: u64 },
    CostCents { limit: u64 },
    Combined { budgets: Vec<Budget> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    Tokens,
    Calls,
    Duration,
    CostCents,
    Combined,
}

impl Budget {
    pub fn tokens(n: u64) -> Self {
        Self::Tokens { limit: n }
    }

    pub fn calls(n: u64) -> Self {
        Self::Calls { limit: n }
    }

    pub fn duration(d: Duration) -> Self {
        Self::Duration {
            limit_ms: d.as_millis() as u64,
        }
    }

    pub fn cost_cents(n: u64) -> Self {
        Self::CostCents { limit: n }
    }

    pub fn combined(budgets: Vec<Budget>) -> Self {
        Self::Combined { budgets }
    }

    pub fn kind(&self) -> BudgetKind {
        match self {
            Self::Tokens { .. } => BudgetKind::Tokens,
            Self::Calls { .. } => BudgetKind::Calls,
            Self::Duration { .. } => BudgetKind::Duration,
            Self::CostCents { .. } => BudgetKind::CostCents,
            Self::Combined { .. } => BudgetKind::Combined,
        }
    }

    pub fn limit(&self) -> u64 {
        match self {
            Self::Tokens { limit }
            | Self::Calls { limit }
            | Self::Duration { limit_ms: limit }
            | Self::CostCents { limit } => *limit,
            Self::Combined { budgets } => budgets.iter().map(|b| b.limit()).sum(),
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::Tokens { limit: u64::MAX }
    }
}

/// Tracks budget consumption at runtime.
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    budget: Budget,
    used: u64,
}

impl BudgetTracker {
    pub fn new(budget: Budget) -> Self {
        Self { budget, used: 0 }
    }

    pub fn remaining(&self) -> u64 {
        self.budget.limit().saturating_sub(self.used)
    }

    pub fn consume(&mut self, amount: u64) {
        self.used += amount;
    }

    pub fn is_exceeded(&self) -> bool {
        self.used > self.budget.limit()
    }

    pub fn budget(&self) -> &Budget {
        &self.budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_tracking() {
        let mut tracker = BudgetTracker::new(Budget::tokens(100));
        assert_eq!(tracker.remaining(), 100);

        tracker.consume(30);
        assert_eq!(tracker.remaining(), 70);
        assert!(!tracker.is_exceeded());

        tracker.consume(80);
        assert!(tracker.is_exceeded());
    }

    #[test]
    fn serde_round_trip() {
        let budget = Budget::combined(vec![
            Budget::tokens(4000),
            Budget::duration(Duration::from_secs(30)),
        ]);
        let json = serde_json::to_string(&budget).unwrap();
        let back: Budget = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind(), BudgetKind::Combined);
    }
}
