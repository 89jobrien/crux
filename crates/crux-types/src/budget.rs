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
            // NOTE: for `Combined`, summing limits across mixed units (e.g. calls +
            // duration_ms) is not meaningful on its own — it exists only as a rough
            // aggregate for display/error-metadata purposes. `BudgetTracker` does NOT
            // use this to decide whether a combined budget is exceeded; it tracks each
            // leaf dimension independently instead. See `BudgetTracker::leaf_limits`.
            Self::Combined { budgets } => budgets.iter().map(|b| b.limit()).sum(),
        }
    }

    /// Flattens this budget into its independent leaf dimensions (kind, limit),
    /// recursing into nested `Combined` budgets. A non-combined budget yields a
    /// single leaf.
    fn leaves(&self) -> Vec<(BudgetKind, u64)> {
        match self {
            Self::Combined { budgets } => budgets.iter().flat_map(Budget::leaves).collect(),
            other => vec![(other.kind(), other.limit())],
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::Tokens { limit: u64::MAX }
    }
}

/// Tracks budget consumption at runtime.
///
/// `Combined` budgets mix units (e.g. a call count and a duration in
/// milliseconds). Summing their limits into one counter would let a
/// large limit in one dimension mask exhaustion in another, so each leaf
/// dimension is tracked (and checked for exceedance) independently.
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    budget: Budget,
    /// Per-dimension (limit, used) pairs, one per leaf of `budget`. For a
    /// non-combined budget this has exactly one entry.
    leaves: Vec<(u64, u64)>,
}

impl BudgetTracker {
    pub fn new(budget: Budget) -> Self {
        let leaves = budget
            .leaves()
            .into_iter()
            .map(|(_, limit)| (limit, 0u64))
            .collect();
        Self { budget, leaves }
    }

    /// Smallest remaining amount across all tracked dimensions. For a single
    /// (non-combined) budget this is exact; for `Combined` it is the
    /// distance to the first dimension that will be exhausted.
    pub fn remaining(&self) -> u64 {
        self.leaves
            .iter()
            .map(|(limit, used)| limit.saturating_sub(*used))
            .min()
            .unwrap_or(0)
    }

    /// Records `amount` of consumption against every tracked dimension.
    /// Callers currently report a single scalar amount of work done (e.g.
    /// one step, one call); that amount is applied uniformly to each
    /// dimension so each one is still checked against its own limit rather
    /// than a meaningless combined total.
    pub fn consume(&mut self, amount: u64) {
        for (_, used) in &mut self.leaves {
            *used = used.saturating_add(amount);
        }
    }

    /// True if ANY tracked dimension has exceeded its own limit.
    pub fn is_exceeded(&self) -> bool {
        self.leaves.iter().any(|(limit, used)| used > limit)
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

    /// Regression test for #91: a `Combined` budget must track each dimension
    /// independently. Previously `BudgetTracker` summed mixed-unit limits into
    /// one counter (e.g. `calls: 5` + `duration_ms: 10_000` -> limit 10_005),
    /// so 9_999 "calls" of consumption would not trip `is_exceeded` even
    /// though the 5-call limit was blown past thousands of units ago.
    #[test]
    fn combined_budget_tracks_dimensions_independently() {
        let combined = Budget::combined(vec![
            Budget::calls(5),
            Budget::duration(Duration::from_millis(10_000)),
        ]);
        let mut tracker = BudgetTracker::new(combined);

        // Buggy behavior: limit() summed to 10_005, so consuming 9 units of
        // "usage" would report far from exceeded. Correct behavior: the
        // `calls` dimension has a limit of 5, so this must already be
        // exceeded once 6+ units have been consumed, regardless of the much
        // larger `duration_ms` limit.
        tracker.consume(6);
        assert!(
            tracker.is_exceeded(),
            "calls dimension (limit 5) should be exceeded independently of duration_ms (limit 10_000)"
        );
    }

    #[test]
    fn combined_budget_not_exceeded_until_a_dimension_is_exceeded() {
        let combined = Budget::combined(vec![
            Budget::calls(5),
            Budget::duration(Duration::from_millis(10_000)),
        ]);
        let mut tracker = BudgetTracker::new(combined);

        tracker.consume(5);
        assert!(!tracker.is_exceeded());

        tracker.consume(1);
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
