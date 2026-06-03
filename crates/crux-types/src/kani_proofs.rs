//! Kani formal verification harnesses for crux-types.
//!
//! Run with: `cargo kani -p crux-types`

use crate::budget::{Budget, BudgetTracker};
use crate::crux_value::FinalPhase;

/// Prove: after saturating_add fix, consume never bypasses is_exceeded.
#[kani::proof]
fn budget_consume_no_overflow_bypass() {
    let limit: u64 = kani::any();
    let used: u64 = kani::any();
    let amount: u64 = kani::any();

    kani::assume(limit < u64::MAX / 2);
    kani::assume(used <= limit);

    let mut tracker = BudgetTracker::new(Budget::tokens(limit));
    tracker.consume(used);
    tracker.consume(amount);

    // With saturating_add, if real total > limit, is_exceeded must be true.
    // Saturation caps at u64::MAX which is always > limit (since limit < MAX/2).
    let total = used.saturating_add(amount);
    if total > limit {
        assert!(tracker.is_exceeded());
    }
}

/// Prove: FinalPhase ordering matches documented severity (ascending).
#[kani::proof]
fn final_phase_severity_ordering() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    kani::assume(a < 5);
    kani::assume(b < 5);

    let phases = [
        FinalPhase::Succeeded,
        FinalPhase::Skipped,
        FinalPhase::Aborted,
        FinalPhase::Failed,
        FinalPhase::Errored,
    ];

    let pa = phases[a as usize];
    let pb = phases[b as usize];

    // Derived Ord should match declaration order
    if a < b {
        assert!(pa < pb);
    } else if a > b {
        assert!(pa > pb);
    } else {
        assert!(pa == pb);
    }
}

/// Prove: BudgetTracker::remaining + used == limit when used <= limit.
#[kani::proof]
fn budget_remaining_plus_used_equals_limit() {
    let limit: u64 = kani::any();
    let used: u64 = kani::any();
    kani::assume(used <= limit);

    let mut tracker = BudgetTracker::new(Budget::tokens(limit));
    tracker.consume(used);

    let remaining = tracker.remaining();
    assert_eq!(remaining + used, limit);
}

/// Prove: is_exceeded returns false when used == limit (edge case documentation).
#[kani::proof]
fn budget_at_exact_limit_not_exceeded() {
    let limit: u64 = kani::any();
    kani::assume(limit > 0);

    let mut tracker = BudgetTracker::new(Budget::tokens(limit));
    tracker.consume(limit);

    // used == limit means NOT exceeded (uses strict >)
    assert!(!tracker.is_exceeded());
    assert_eq!(tracker.remaining(), 0);
}
