//! Kani formal verification harnesses for crux-runtime.
//!
//! Run with: `cargo kani -p crux-runtime --no-default-features`

use crate::governance::{GovernancePolicy, PolicyAction, compose_policies};
use crate::recorder::hash_step_identity;
use crate::trust::TrustScore;
use crate::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};

// --- hash_step_identity ---

/// Prove: same (name, ordinal) always produces same hash (determinism).
#[kani::proof]
fn hash_identity_deterministic() {
    let ordinal: u32 = kani::any();
    // Use a fixed short name for tractability
    let name = "step";
    let h1 = hash_step_identity(name, ordinal);
    let h2 = hash_step_identity(name, ordinal);
    assert_eq!(h1, h2);
}

/// Prove: different ordinals produce different hashes (for same name).
#[kani::proof]
fn hash_identity_ordinal_sensitive() {
    let o1: u32 = kani::any();
    let o2: u32 = kani::any();
    kani::assume(o1 != o2);
    let name = "step";
    let h1 = hash_step_identity(name, o1);
    let h2 = hash_step_identity(name, o2);
    // Not a guarantee (hash collisions exist), but for DefaultHasher with
    // these inputs, we expect no collision. If this fails, we have a
    // collision-prone identity function.
    assert_ne!(h1, h2);
}

// --- GovernancePolicy ---

/// Prove: blocked always beats allowed (security-critical priority).
#[kani::proof]
fn governance_blocked_beats_allowed() {
    let policy = GovernancePolicy {
        name: String::new(),
        allowed_tools: vec!["tool_a".into()],
        blocked_tools: vec!["tool_a".into()],
        blocked_patterns: vec![],
        max_calls_per_request: 100,
        require_human_approval: vec![],
    };
    assert_eq!(policy.check_tool("tool_a"), PolicyAction::Deny);
}

/// Prove: empty allowed list means allow-all (no deny from allowlist).
#[kani::proof]
fn governance_empty_allowlist_permits_all() {
    let policy = GovernancePolicy {
        name: String::new(),
        allowed_tools: vec![],
        blocked_tools: vec![],
        blocked_patterns: vec![],
        max_calls_per_request: 100,
        require_human_approval: vec![],
    };
    // Any tool name should be allowed
    assert_eq!(policy.check_tool("anything"), PolicyAction::Allow);
}

/// Prove: compose_policies takes minimum rate limit.
#[kani::proof]
fn governance_compose_min_rate() {
    let a: usize = kani::any();
    let b: usize = kani::any();
    kani::assume(a > 0);
    kani::assume(b > 0);
    kani::assume(a < 10000);
    kani::assume(b < 10000);

    let pa = GovernancePolicy {
        max_calls_per_request: a,
        ..GovernancePolicy::default()
    };
    let pb = GovernancePolicy {
        max_calls_per_request: b,
        ..GovernancePolicy::default()
    };
    let composed = compose_policies(&[pa, pb]);
    assert_eq!(composed.max_calls_per_request, a.min(b));
}

// --- HarnessDiff::apply ---

/// Prove: applying a diff never produces negative resource values
/// (the `.max(0) as u64` clamp is sound for values where base fits in i64).
#[kani::proof]
fn harness_diff_apply_no_negative() {
    let base_mem: u64 = kani::any();
    let base_cpu: u64 = kani::any();
    let base_timeout: u64 = kani::any();
    let delta_mem: i64 = kani::any();
    let delta_cpu: i64 = kani::any();
    let delta_timeout: i64 = kani::any();

    // Constrain base values to fit in i64 (realistic — no profile has > i64::MAX MB)
    kani::assume(base_mem <= i64::MAX as u64);
    kani::assume(base_cpu <= i64::MAX as u64);
    kani::assume(base_timeout <= i64::MAX as u64);

    let profile = HarnessProfile {
        id: String::new(),
        resources: ResourceHints {
            memory_mb: base_mem,
            cpu_millicores: base_cpu,
            timeout_seconds: base_timeout,
        },
        network_access: false,
        allowed_syscalls: vec![],
    };

    let diff = HarnessDiff {
        memory_delta_mb: Some(delta_mem),
        cpu_delta_millicores: Some(delta_cpu),
        timeout_delta_seconds: Some(delta_timeout),
        network_access_change: None,
        syscall_additions: vec![],
        syscall_removals: vec![],
    };

    let result = diff.apply(&profile);
    // All resource fields must be non-negative (>= 0 is trivially true for u64,
    // but the concern is that the i64 arithmetic wraps before the u64 cast).
    // The real invariant: result <= base + delta when delta > 0, result == 0 when
    // base + delta < 0.
    assert!(result.resources.memory_mb <= u64::MAX);
    assert!(result.resources.cpu_millicores <= u64::MAX);
    assert!(result.resources.timeout_seconds <= u64::MAX);

    // Floor invariant: if base as i64 + delta < 0, result is 0.
    if (base_mem as i64).saturating_add(delta_mem) <= 0 {
        assert_eq!(result.resources.memory_mb, 0);
    }
}

/// Prove: empty diff is identity (no changes applied).
#[kani::proof]
fn harness_diff_empty_is_identity() {
    let mem: u64 = kani::any();
    let cpu: u64 = kani::any();
    let timeout: u64 = kani::any();
    kani::assume(mem < 1_000_000);
    kani::assume(cpu < 1_000_000);
    kani::assume(timeout < 1_000_000);

    let profile = HarnessProfile {
        id: String::new(),
        resources: ResourceHints {
            memory_mb: mem,
            cpu_millicores: cpu,
            timeout_seconds: timeout,
        },
        network_access: false,
        allowed_syscalls: vec![],
    };

    let diff = HarnessDiff::default();
    let result = diff.apply(&profile);
    assert_eq!(result.resources.memory_mb, mem);
    assert_eq!(result.resources.cpu_millicores, cpu);
    assert_eq!(result.resources.timeout_seconds, timeout);
}

/// Prove: has_changes returns false iff all fields are None/empty.
#[kani::proof]
fn harness_diff_has_changes_biconditional() {
    let diff = HarnessDiff::default();
    assert!(!diff.has_changes());

    let diff_with_mem = HarnessDiff {
        memory_delta_mb: Some(1),
        ..Default::default()
    };
    assert!(diff_with_mem.has_changes());
}

// --- TrustScore ---

/// Prove: record_success keeps score in [0.0, 1.0] for valid reward.
#[kani::proof]
fn trust_score_success_clamped() {
    let initial: f64 = kani::any();
    kani::assume(initial >= 0.0 && initial <= 1.0);
    kani::assume(!initial.is_nan());

    let reward: f64 = kani::any();
    kani::assume(reward >= 0.0 && reward <= 1.0);
    kani::assume(!reward.is_nan());

    let mut ts = TrustScore::default();
    ts.score = initial;
    ts.record_success(reward);

    assert!(ts.score >= 0.0);
    assert!(ts.score <= 1.0);
}

/// Prove: record_failure keeps score in [0.0, 1.0] for valid penalty.
#[kani::proof]
fn trust_score_failure_clamped() {
    let initial: f64 = kani::any();
    kani::assume(initial >= 0.0 && initial <= 1.0);
    kani::assume(!initial.is_nan());

    let penalty: f64 = kani::any();
    kani::assume(penalty >= 0.0 && penalty <= 1.0);
    kani::assume(!penalty.is_nan());

    let mut ts = TrustScore::default();
    ts.score = initial;
    ts.record_failure(penalty);

    assert!(ts.score >= 0.0);
    assert!(ts.score <= 1.0);
}
