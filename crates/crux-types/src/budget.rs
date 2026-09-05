/// Budget constraints for agent execution.
use serde::{Deserialize, Deserializer, Serialize, de};
use std::time::Duration;

use crate::error::CruxErr;

/// A non-negative USD amount stored as integer microdollars.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct UsdAmount {
    micros: u64,
}

impl UsdAmount {
    pub const ZERO: Self = Self { micros: 0 };

    pub const fn from_micros(micros: u64) -> Self {
        Self { micros }
    }

    pub const fn micros(self) -> u64 {
        self.micros
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.micros.checked_add(other.micros).map(Self::from_micros)
    }
}

impl<'de> Deserialize<'de> for UsdAmount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let amount = f64::deserialize(deserializer)?;
        if !amount.is_finite() || amount.is_sign_negative() {
            return Err(de::Error::custom(
                "USD budget must be finite and non-negative",
            ));
        }

        let micros = amount * 1_000_000.0;
        let rounded = micros.round();
        if rounded > u64::MAX as f64 {
            return Err(de::Error::custom("USD budget exceeds supported range"));
        }
        if (micros - rounded).abs() > 1e-6 {
            return Err(de::Error::custom(
                "USD budget supports at most six decimal places",
            ));
        }
        Ok(Self::from_micros(rounded as u64))
    }
}

impl std::fmt::Display for UsdAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "${}.{:06}",
            self.micros / 1_000_000,
            self.micros % 1_000_000
        )
    }
}

/// Usage reported by one handler invocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HandlerUsage {
    pub tokens: u64,
    /// `None` means unreported; `Some(UsdAmount::ZERO)` means explicitly free.
    pub usd: Option<UsdAmount>,
}

impl HandlerUsage {
    pub const fn free() -> Self {
        Self {
            tokens: 0,
            usd: Some(UsdAmount::ZERO),
        }
    }

    pub const fn metered(tokens: u64, usd: UsdAmount) -> Self {
        Self {
            tokens,
            usd: Some(usd),
        }
    }

    pub const fn unreported() -> Self {
        Self {
            tokens: 0,
            usd: None,
        }
    }
}

/// Aggregate usage recorded against a budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BudgetUsage {
    pub steps: u64,
    pub tokens: u64,
    pub duration_ms: u64,
    pub usd: Option<UsdAmount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Budget {
    Tokens { limit: u64 },
    Steps { limit: u64 },
    Calls { limit: u64 },
    Duration { limit_ms: u64 },
    Usd { limit_micros: u64 },
    CostCents { limit: u64 },
    Combined { budgets: Vec<Budget> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    Tokens,
    Steps,
    Calls,
    Duration,
    Usd,
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

    pub fn steps(n: u64) -> Self {
        Self::Steps { limit: n }
    }

    pub fn duration(d: Duration) -> Self {
        Self::Duration {
            limit_ms: d.as_millis() as u64,
        }
    }

    pub fn cost_cents(n: u64) -> Self {
        Self::CostCents { limit: n }
    }

    pub fn usd(amount: UsdAmount) -> Self {
        Self::Usd {
            limit_micros: amount.micros(),
        }
    }

    pub fn combined(budgets: Vec<Budget>) -> Self {
        Self::Combined { budgets }
    }

    pub fn kind(&self) -> BudgetKind {
        match self {
            Self::Tokens { .. } => BudgetKind::Tokens,
            Self::Steps { .. } => BudgetKind::Steps,
            Self::Calls { .. } => BudgetKind::Calls,
            Self::Duration { .. } => BudgetKind::Duration,
            Self::Usd { .. } => BudgetKind::Usd,
            Self::CostCents { .. } => BudgetKind::CostCents,
            Self::Combined { .. } => BudgetKind::Combined,
        }
    }

    pub fn limit(&self) -> u64 {
        match self {
            Self::Tokens { limit }
            | Self::Steps { limit }
            | Self::Calls { limit }
            | Self::Duration { limit_ms: limit }
            | Self::Usd {
                limit_micros: limit,
            }
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
            Self::Calls { limit } => vec![(BudgetKind::Steps, *limit)],
            Self::CostCents { limit } => {
                vec![(BudgetKind::Usd, limit.saturating_mul(10_000))]
            }
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
    counters: Vec<BudgetCounter>,
    usage: BudgetUsage,
}

#[derive(Debug, Clone)]
struct BudgetCounter {
    kind: BudgetKind,
    limit: u64,
    used: u64,
}

impl BudgetTracker {
    pub fn new(budget: Budget) -> Self {
        let counters = budget
            .leaves()
            .into_iter()
            .map(|(kind, limit)| BudgetCounter {
                kind,
                limit,
                used: 0,
            })
            .collect();
        Self {
            budget,
            counters,
            usage: BudgetUsage::default(),
        }
    }

    /// Smallest remaining amount across all tracked dimensions. For a single
    /// (non-combined) budget this is exact; for `Combined` it is the
    /// distance to the first dimension that will be exhausted.
    pub fn remaining(&self) -> u64 {
        self.counters
            .iter()
            .map(|counter| counter.limit.saturating_sub(counter.used))
            .min()
            .unwrap_or(0)
    }

    pub fn begin_step(&mut self) -> Result<(), CruxErr> {
        let attempted = self.usage.steps.saturating_add(1);
        if let Some(counter) = self
            .counters
            .iter_mut()
            .find(|counter| counter.kind == BudgetKind::Steps)
        {
            if attempted > counter.limit {
                return Err(CruxErr::StepBudgetExceeded {
                    limit: counter.limit,
                    attempted,
                });
            }
            counter.used = attempted;
        }
        self.usage.steps = attempted;
        Ok(())
    }

    pub fn record_handler_usage(&mut self, step: &str, usage: HandlerUsage) -> Result<(), CruxErr> {
        let has_usd_budget = self
            .counters
            .iter()
            .any(|counter| counter.kind == BudgetKind::Usd);
        if has_usd_budget && usage.usd.is_none() {
            return Err(CruxErr::UnreportedCost {
                step: step.to_string(),
                source: None,
            });
        }

        self.usage.tokens = self.usage.tokens.saturating_add(usage.tokens);
        if let Some(counter) = self
            .counters
            .iter_mut()
            .find(|counter| counter.kind == BudgetKind::Tokens)
        {
            counter.used = self.usage.tokens;
            if counter.used > counter.limit {
                return Err(CruxErr::BudgetExceeded {
                    budget_kind: BudgetKind::Tokens,
                    limit: counter.limit,
                    actual: counter.used,
                });
            }
        }

        if let Some(amount) = usage.usd {
            let current = self.usage.usd.unwrap_or(UsdAmount::ZERO);
            let total = current
                .checked_add(amount)
                .ok_or(CruxErr::UsdBudgetExceeded {
                    limit_micros: u64::MAX,
                    actual_micros: u64::MAX,
                    source: None,
                })?;
            self.usage.usd = Some(total);
            if let Some(counter) = self
                .counters
                .iter_mut()
                .find(|counter| counter.kind == BudgetKind::Usd)
            {
                counter.used = total.micros();
                if counter.used > counter.limit {
                    return Err(CruxErr::UsdBudgetExceeded {
                        limit_micros: counter.limit,
                        actual_micros: counter.used,
                        source: None,
                    });
                }
            }
        }
        Ok(())
    }

    pub fn record_duration(&mut self, duration: Duration) -> Result<(), CruxErr> {
        let millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        self.usage.duration_ms = self.usage.duration_ms.saturating_add(millis);
        if let Some(counter) = self
            .counters
            .iter_mut()
            .find(|counter| counter.kind == BudgetKind::Duration)
        {
            counter.used = self.usage.duration_ms;
            if counter.used > counter.limit {
                return Err(CruxErr::BudgetExceeded {
                    budget_kind: BudgetKind::Duration,
                    limit: counter.limit,
                    actual: counter.used,
                });
            }
        }
        Ok(())
    }

    pub fn usage(&self) -> BudgetUsage {
        self.usage
    }

    /// Records `amount` of consumption against every tracked dimension.
    /// Callers currently report a single scalar amount of work done (e.g.
    /// one step, one call); that amount is applied uniformly to each
    /// dimension so each one is still checked against its own limit rather
    /// than a meaningless combined total.
    pub fn consume(&mut self, amount: u64) {
        for counter in &mut self.counters {
            counter.used = counter.used.saturating_add(amount);
        }
    }

    /// True if ANY tracked dimension has exceeded its own limit.
    pub fn is_exceeded(&self) -> bool {
        self.counters
            .iter()
            .any(|counter| counter.used > counter.limit)
    }

    pub fn budget(&self) -> &Budget {
        &self.budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usd_amount_deserializes_to_microdollars() {
        let amount: UsdAmount = serde_json::from_str("1.25").unwrap();
        assert_eq!(amount.micros(), 1_250_000);
        assert_eq!(amount.to_string(), "$1.250000");
    }

    #[test]
    fn handler_usage_distinguishes_free_from_unreported() {
        assert_eq!(HandlerUsage::free().usd, Some(UsdAmount::ZERO));
        assert_eq!(HandlerUsage::unreported().usd, None);
    }

    #[test]
    fn step_budget_allows_limit_and_rejects_next_attempt() {
        let mut tracker = BudgetTracker::new(Budget::steps(2));
        assert!(tracker.begin_step().is_ok());
        assert!(tracker.begin_step().is_ok());
        assert!(matches!(
            tracker.begin_step(),
            Err(CruxErr::StepBudgetExceeded {
                limit: 2,
                attempted: 3
            })
        ));
    }

    #[test]
    fn usd_budget_fails_closed_and_allows_exact_limit() {
        let mut tracker = BudgetTracker::new(Budget::usd(UsdAmount::from_micros(100)));
        assert!(matches!(
            tracker.record_handler_usage("paid", HandlerUsage::unreported()),
            Err(CruxErr::UnreportedCost { .. })
        ));
        assert!(
            tracker
                .record_handler_usage(
                    "paid",
                    HandlerUsage::metered(0, UsdAmount::from_micros(100))
                )
                .is_ok()
        );
        assert!(matches!(
            tracker
                .record_handler_usage("paid", HandlerUsage::metered(0, UsdAmount::from_micros(1))),
            Err(CruxErr::UsdBudgetExceeded {
                limit_micros: 100,
                actual_micros: 101,
                ..
            })
        ));
    }

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
