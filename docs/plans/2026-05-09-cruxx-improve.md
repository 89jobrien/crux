# Plan: praxis — Self-Improving Agent Runtime

## Goal

Create `~/dev/praxis` as a standalone Rust workspace that closes the
disconnected loops between crux execution traces, devloop council
evaluation, magi/looprs scoring, and strategy evolution — enabling
agents to improve their own behavior across sessions.

## Architecture

Two projects, two responsibilities:

1. **`crux-improve`** (new crate in `~/dev/crux` workspace) — shared
   vocabulary + bridge layer between crux and any improvement consumer.
   Like `slashcrux` is for slash/crux, `crux-improve` is for
   crux/praxis. Owns:
   - **`TraceMetrics`** — extracted from a `Crux<T>`: success rate, avg
     confidence, duration, step count, delegation depth, speculation hit
     rate, error distribution. Crux-domain knowledge.
   - **`Verdict` + `Comparison`** — comparing two traces is a crux
     concern (it knows what steps mean). `replay_compare` lives here.
   - **`ImprovementKind` + `StrategyDiff` + `Strategy`** — the nouns of
     the improvement protocol. Shared vocabulary.
   - **`StrategyPolicy`** — extends `SafetyPolicy` to validate
     `StrategyDiff` (not just `HarnessDiff`).
   - **`EvolutionStrategyPlanner`** — adapter wrapping `EvolutionPlanner`
     into praxis's `StrategyPlanner` trait.
   - Re-exports of `Crux<T>`, `Step`, `CruxId`, `SafetyPolicy`,
     `HarnessDiff`, `RunMetrics`.

2. **`~/dev/praxis`** (new standalone workspace) — the improvement loop.
   Depends on `crux-improve` as its single crux dependency. Owns:
   - Port traits: `Evaluator`, `StrategyPlanner`, `StrategyStore`,
     `RewardAccumulator`
   - Adapters: `StubEvaluator`, `DeterministicStrategyPlanner`,
     `FileStrategyStore`, `InMemoryRewardStore`
   - Orchestrator: `ImprovementLoop`

```
~/dev/crux/crates/
  crux-improve/
    src/
      lib.rs               # re-exports + module declarations
      metrics.rs           # TraceMetrics extraction from Crux<T>
      comparison.rs        # replay_compare, Verdict, Comparison
      improvement.rs       # ImprovementKind, StrategyDiff, Strategy
      policy.rs            # StrategyPolicy (extends SafetyPolicy)
      evolution_adapter.rs # EvolutionPlanner -> StrategyPlanner bridge

~/dev/praxis/
  crates/
    praxis-core/           # port traits (Evaluator, StrategyPlanner, etc.)
    praxis-eval/           # evaluator + planner adapters
    praxis-store/          # storage adapters
    praxis/                # ImprovementLoop orchestrator
```

**Dependency direction:**

```
praxis -> crux-improve -> crux-runtime, crux-types, crux-planner
```

Praxis never imports `crux-runtime`, `crux-types`, or `crux-planner`
directly — `crux-improve` is the single entry point.

## Tech Stack

- Rust edition 2024, MSRV 1.85 (matching crux)
- `crux-improve`: depends on `crux-runtime`, `crux-types`,
  `crux-planner` (workspace path deps inside crux)
- `praxis`: depends on `crux-improve` (git dep from `~/dev/crux`)
- Shared: `serde`, `serde_json`, `chrono`, `thiserror`, `async-trait`
- `praxis-store`: `rusqlite` (bundled) for `SqliteRewardStore`
- `praxis`: `tokio`, `clap` for CLI
- Dev deps: `tokio`, `insta`, `tempfile`
- License: MIT OR Apache-2.0

## Tasks

### Task 0: crux-improve — TraceMetrics and comparison (in crux workspace)

**Crate**: `crux-improve`
**File(s)**: `crates/crux-improve/Cargo.toml`,
`crates/crux-improve/src/lib.rs`,
`crates/crux-improve/src/metrics.rs`,
`crates/crux-improve/src/comparison.rs`
**Run**: `cargo nextest run -p crux-improve`

1. Write failing test:

   ```rust
   // crates/crux-improve/src/metrics.rs
   #[cfg(test)]
   mod tests {
       use super::*;
       use crux_types::crux_value::Crux;
       use crux_types::id::CruxId;
       use crux_types::step::{Step, StepKind, StepStatus};
       use chrono::Utc;

       fn step(name: &str, status: StepStatus, confidence: f32, kind: StepKind) -> Step {
           Step {
               name: name.into(),
               kind,
               status,
               confidence,
               started_at: Utc::now(),
               duration_ms: 100,
               input_hash: 0,
               content_hash: None,
               output: None,
               error: None,
               attempt: 1,
               events: vec![],
               metadata: Default::default(),
           }
       }

       fn trace(steps: Vec<Step>, children: Vec<Crux<serde_json::Value>>) -> Crux<serde_json::Value> {
           Crux {
               id: CruxId::new(),
               agent: "test".into(),
               value: Ok(serde_json::json!({})),
               steps,
               children,
               started_at: Utc::now(),
               finished_at: Some(Utc::now()),
           }
       }

       #[test]
       fn empty_trace_metrics() {
           let m = TraceMetrics::extract(&trace(vec![], vec![]));
           assert_eq!(m.step_count, 0);
           assert!((m.score - 0.5).abs() < f32::EPSILON);
           assert_eq!(m.delegation_depth, 0);
       }

       #[test]
       fn success_rate_computed_correctly() {
           let t = trace(vec![
               step("a", StepStatus::Ok, 0.8, StepKind::Plain),
               step("b", StepStatus::Err, 0.3, StepKind::Plain),
               step("c", StepStatus::Ok, 0.9, StepKind::Plain),
           ], vec![]);
           let m = TraceMetrics::extract(&t);
           assert!((m.success_rate - 2.0 / 3.0).abs() < 0.01);
           assert_eq!(m.error_count, 1);
       }

       #[test]
       fn delegation_depth_counts_nesting() {
           let child = trace(vec![
               step("inner", StepStatus::Ok, 0.7, StepKind::Plain),
           ], vec![]);
           let parent = trace(vec![
               step("outer", StepStatus::Ok, 0.8, StepKind::Delegation),
           ], vec![child]);
           let m = TraceMetrics::extract(&parent);
           assert_eq!(m.delegation_depth, 1);
           assert_eq!(m.delegation_count, 1);
       }

       #[test]
       fn speculation_stats() {
           let t = trace(vec![
               step("spec-a", StepStatus::Ok, 0.9, StepKind::Speculation),
               step("spec-b", StepStatus::Rejected, 0.4, StepKind::Speculation),
               step("plain", StepStatus::Ok, 0.7, StepKind::Plain),
           ], vec![]);
           let m = TraceMetrics::extract(&t);
           assert_eq!(m.speculation_count, 2);
           assert_eq!(m.speculation_hit_count, 1);
           assert!((m.speculation_hit_rate - 0.5).abs() < f32::EPSILON);
       }

       #[test]
       fn metrics_serializable() {
           let m = TraceMetrics::extract(&trace(vec![], vec![]));
           let json = serde_json::to_string(&m).unwrap();
           let back: TraceMetrics = serde_json::from_str(&json).unwrap();
           assert_eq!(back.step_count, 0);
       }
   }
   ```

   ```rust
   // crates/crux-improve/src/comparison.rs
   #[cfg(test)]
   mod tests {
       use super::*;
       use crate::metrics::tests::{step, trace};
       use crux_types::step::{StepKind, StepStatus};

       #[test]
       fn detects_improvement() {
           let old = trace(vec![
               step("a", StepStatus::Err, 0.3, StepKind::Plain),
           ], vec![]);
           let new = trace(vec![
               step("a", StepStatus::Ok, 0.8, StepKind::Plain),
           ], vec![]);
           let cmp = replay_compare(&old, &new);
           assert_eq!(cmp.verdict, Verdict::Improved);
           assert!(cmp.delta > 0.0);
       }

       #[test]
       fn detects_regression() {
           let old = trace(vec![
               step("a", StepStatus::Ok, 0.9, StepKind::Plain),
           ], vec![]);
           let new = trace(vec![
               step("a", StepStatus::Err, 0.2, StepKind::Plain),
           ], vec![]);
           let cmp = replay_compare(&old, &new);
           assert_eq!(cmp.verdict, Verdict::Regressed);
       }

       #[test]
       fn detects_neutral() {
           let old = trace(vec![
               step("a", StepStatus::Ok, 0.7, StepKind::Plain),
           ], vec![]);
           let new = trace(vec![
               step("a", StepStatus::Ok, 0.72, StepKind::Plain),
           ], vec![]);
           let cmp = replay_compare(&old, &new);
           assert_eq!(cmp.verdict, Verdict::Neutral);
       }

       #[test]
       fn comparison_includes_metric_deltas() {
           let old = trace(vec![
               step("a", StepStatus::Ok, 0.5, StepKind::Plain),
               step("b", StepStatus::Err, 0.3, StepKind::Plain),
           ], vec![]);
           let new = trace(vec![
               step("a", StepStatus::Ok, 0.9, StepKind::Plain),
               step("b", StepStatus::Ok, 0.8, StepKind::Plain),
           ], vec![]);
           let cmp = replay_compare(&old, &new);
           assert!(cmp.new_metrics.success_rate > cmp.old_metrics.success_rate);
           assert!(cmp.new_metrics.avg_confidence > cmp.old_metrics.avg_confidence);
       }
   }
   ```

   Run: `cargo nextest run -p crux-improve`
   Expected: FAIL

2. Create `crates/crux-improve/Cargo.toml`:

   ```toml
   [package]
   name = "crux-improve"
   version.workspace = true
   edition.workspace = true
   rust-version.workspace = true
   license.workspace = true
   description = "Shared vocabulary and bridge types for crux self-improvement"

   [dependencies]
   crux-runtime = { path = "../crux-runtime", version = "0.2.5" }
   crux-types = { path = "../crux-types", version = "0.2.5" }
   crux-planner = { path = "../crux-planner", version = "0.2.5" }
   serde = { workspace = true }
   serde_json = { workspace = true }
   chrono = { workspace = true }
   thiserror = { workspace = true }

   [dev-dependencies]
   tokio = { workspace = true }
   ```

3. Add `"crates/crux-improve"` to workspace members in crux root
   `Cargo.toml`.

4. Implement `crates/crux-improve/src/metrics.rs`:

   ```rust
   use crux_types::crux_value::Crux;
   use crux_types::step::{StepKind, StepStatus};
   use serde::{Deserialize, Serialize};

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct TraceMetrics {
       pub step_count: usize,
       pub success_rate: f32,
       pub error_count: usize,
       pub avg_confidence: f32,
       pub total_duration_ms: u64,
       pub delegation_count: usize,
       pub delegation_depth: usize,
       pub speculation_count: usize,
       pub speculation_hit_count: usize,
       pub speculation_hit_rate: f32,
       pub score: f32,
   }

   impl TraceMetrics {
       pub fn extract(trace: &Crux<serde_json::Value>) -> Self {
           let steps = &trace.steps;
           let step_count = steps.len();

           if step_count == 0 {
               return Self {
                   step_count: 0,
                   success_rate: 1.0,
                   error_count: 0,
                   avg_confidence: 0.0,
                   total_duration_ms: 0,
                   delegation_count: 0,
                   delegation_depth: 0,
                   speculation_count: 0,
                   speculation_hit_count: 0,
                   speculation_hit_rate: 0.0,
                   score: 0.5,
               };
           }

           let ok_count = steps.iter()
               .filter(|s| s.status == StepStatus::Ok)
               .count();
           let error_count = steps.iter()
               .filter(|s| s.status == StepStatus::Err)
               .count();
           let success_rate = ok_count as f32 / step_count as f32;

           let avg_confidence = steps.iter()
               .map(|s| s.confidence)
               .sum::<f32>() / step_count as f32;

           let total_duration_ms = steps.iter()
               .map(|s| s.duration_ms)
               .sum();

           let delegation_count = steps.iter()
               .filter(|s| s.kind == StepKind::Delegation)
               .count();

           let delegation_depth = Self::max_depth(trace);

           let speculation_count = steps.iter()
               .filter(|s| s.kind == StepKind::Speculation)
               .count();
           let speculation_hit_count = steps.iter()
               .filter(|s| s.kind == StepKind::Speculation && s.status == StepStatus::Ok)
               .count();
           let speculation_hit_rate = if speculation_count > 0 {
               speculation_hit_count as f32 / speculation_count as f32
           } else {
               0.0
           };

           let score = success_rate * 0.6 + avg_confidence * 0.4;

           Self {
               step_count,
               success_rate,
               error_count,
               avg_confidence,
               total_duration_ms,
               delegation_count,
               delegation_depth,
               speculation_count,
               speculation_hit_count,
               speculation_hit_rate,
               score,
           }
       }

       fn max_depth(trace: &Crux<serde_json::Value>) -> usize {
           if trace.children.is_empty() {
               return 0;
           }
           trace.children.iter()
               .map(|c| 1 + Self::max_depth(c))
               .max()
               .unwrap_or(0)
       }
   }

   // Make test helpers available to comparison.rs tests
   #[cfg(test)]
   pub(crate) mod tests {
       use super::*;
       use crux_types::crux_value::Crux;
       use crux_types::id::CruxId;
       use crux_types::step::{Step, StepKind, StepStatus};
       use chrono::Utc;

       pub fn step(name: &str, status: StepStatus, confidence: f32, kind: StepKind) -> Step {
           Step {
               name: name.into(),
               kind,
               status,
               confidence,
               started_at: Utc::now(),
               duration_ms: 100,
               input_hash: 0,
               content_hash: None,
               output: None,
               error: None,
               attempt: 1,
               events: vec![],
               metadata: Default::default(),
           }
       }

       pub fn trace(steps: Vec<Step>, children: Vec<Crux<serde_json::Value>>) -> Crux<serde_json::Value> {
           Crux {
               id: CruxId::new(),
               agent: "test".into(),
               value: Ok(serde_json::json!({})),
               steps,
               children,
               started_at: Utc::now(),
               finished_at: Some(Utc::now()),
           }
       }

       // ... (tests from step 1 above)
   }
   ```

5. Implement `crates/crux-improve/src/comparison.rs`:

   ```rust
   use crate::metrics::TraceMetrics;
   use crux_types::crux_value::Crux;
   use serde::{Deserialize, Serialize};

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   #[serde(rename_all = "snake_case")]
   pub enum Verdict {
       Improved,
       Regressed,
       Neutral,
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Comparison {
       pub verdict: Verdict,
       pub delta: f32,
       pub old_metrics: TraceMetrics,
       pub new_metrics: TraceMetrics,
   }

   pub fn replay_compare(
       old: &Crux<serde_json::Value>,
       new: &Crux<serde_json::Value>,
   ) -> Comparison {
       let old_metrics = TraceMetrics::extract(old);
       let new_metrics = TraceMetrics::extract(new);
       let delta = new_metrics.score - old_metrics.score;

       let verdict = if delta > 0.05 {
           Verdict::Improved
       } else if delta < -0.05 {
           Verdict::Regressed
       } else {
           Verdict::Neutral
       };

       Comparison { verdict, delta, old_metrics, new_metrics }
   }
   ```

6. Verify:

   ```
   cargo nextest run -p crux-improve           -> all green
   cargo clippy -p crux-improve -- -D warnings -> zero warnings
   ```

7. Commit: `git commit -m "feat(crux-improve): add TraceMetrics extraction and replay comparison"`

---

### Task 1: crux-improve — shared improvement vocabulary + StrategyPolicy

**Crate**: `crux-improve`
**File(s)**: `crates/crux-improve/src/improvement.rs`,
`crates/crux-improve/src/policy.rs`,
`crates/crux-improve/src/evolution_adapter.rs`,
`crates/crux-improve/src/lib.rs`
**Run**: `cargo nextest run -p crux-improve`

1. Write failing test:

   ```rust
   // crates/crux-improve/src/improvement.rs
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn kind_serializes_as_snake_case() {
           let kind = ImprovementKind::ToolPreference;
           assert_eq!(serde_json::to_string(&kind).unwrap(), r#""tool_preference""#);
       }

       #[test]
       fn strategy_diff_default_has_no_changes() {
           assert!(!StrategyDiff::default().has_changes());
       }

       #[test]
       fn strategy_diff_with_tool_pref_has_changes() {
           let d = StrategyDiff {
               tool_preferences: vec![("rg".into(), 10)],
               ..Default::default()
           };
           assert!(d.has_changes());
       }

       #[test]
       fn strategy_apply_accumulates() {
           let mut s = Strategy::default();
           let d = StrategyDiff {
               tool_preferences: vec![("rg".into(), 5)],
               ..Default::default()
           };
           s.apply(&d);
           assert_eq!(s.tool_preferences["rg"], 5);
           assert_eq!(s.version, 1);
           s.apply(&d);
           assert_eq!(s.tool_preferences["rg"], 10);
           assert_eq!(s.version, 2);
       }

       #[test]
       fn improvement_is_serializable() {
           let imp = Improvement {
               id: crux_types::id::CruxId::new(),
               kind: ImprovementKind::ConfidenceThreshold,
               target: "agent-a".into(),
               diff: StrategyDiff::default(),
               confidence: 0.8,
               evidence: vec!["finding".into()],
               proposed_at: chrono::Utc::now(),
           };
           let json = serde_json::to_string(&imp).unwrap();
           let back: Improvement = serde_json::from_str(&json).unwrap();
           assert_eq!(back.target, "agent-a");
       }
   }
   ```

   ```rust
   // crates/crux-improve/src/policy.rs
   #[cfg(test)]
   mod tests {
       use super::*;
       use crate::improvement::StrategyDiff;

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
       fn default_policy_requires_approval_for_prompt_patches() {
           let policy = DefaultStrategyPolicy::default();
           let diff = StrategyDiff {
               prompt_patches: vec![crate::improvement::PromptPatch {
                   agent: "a".into(),
                   section: "system".into(),
                   content: "new prompt".into(),
               }],
               ..Default::default()
           };
           assert!(policy.requires_strategy_approval(&diff));
       }

       #[test]
       fn default_policy_does_not_require_approval_for_thresholds() {
           let policy = DefaultStrategyPolicy::default();
           let diff = StrategyDiff {
               confidence_thresholds: vec![("spec".into(), 0.7)],
               ..Default::default()
           };
           assert!(!policy.requires_strategy_approval(&diff));
       }
   }
   ```

   Run: `cargo nextest run -p crux-improve`
   Expected: FAIL

2. Implement `crates/crux-improve/src/improvement.rs`:

   ```rust
   use std::collections::HashMap;
   use chrono::{DateTime, Utc};
   use serde::{Deserialize, Serialize};
   use crux_types::id::CruxId;
   use crux_runtime::types::harness::HarnessDiff;

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   #[serde(rename_all = "snake_case")]
   pub enum ImprovementKind {
       Resource,
       ToolPreference,
       DecompositionStrategy,
       DelegationPolicy,
       PromptTemplate,
       ConfidenceThreshold,
   }

   #[derive(Debug, Clone, Serialize, Deserialize, Default)]
   pub struct StrategyDiff {
       pub tool_preferences: Vec<(String, i32)>,
       pub confidence_thresholds: Vec<(String, f32)>,
       pub delegation_rules: Vec<DelegationRule>,
       pub prompt_patches: Vec<PromptPatch>,
       pub harness_diff: Option<HarnessDiff>,
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct DelegationRule {
       pub pattern: String,
       pub min_steps: u32,
       pub action: DelegationAction,
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   #[serde(rename_all = "snake_case")]
   pub enum DelegationAction {
       Delegate,
       Inline,
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct PromptPatch {
       pub agent: String,
       pub section: String,
       pub content: String,
   }

   impl StrategyDiff {
       pub fn has_changes(&self) -> bool {
           !self.tool_preferences.is_empty()
               || !self.confidence_thresholds.is_empty()
               || !self.delegation_rules.is_empty()
               || !self.prompt_patches.is_empty()
               || self.harness_diff.as_ref().is_some_and(|d| d.has_changes())
       }
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Improvement {
       pub id: CruxId,
       pub kind: ImprovementKind,
       pub target: String,
       pub diff: StrategyDiff,
       pub confidence: f32,
       pub evidence: Vec<String>,
       pub proposed_at: DateTime<Utc>,
   }

   #[derive(Debug, Clone, Serialize, Deserialize, Default)]
   pub struct Strategy {
       pub version: u64,
       pub tool_preferences: HashMap<String, i32>,
       pub confidence_thresholds: HashMap<String, f32>,
       pub delegation_rules: Vec<DelegationRule>,
       pub prompt_overrides: HashMap<String, String>,
       pub updated_at: Option<DateTime<Utc>>,
   }

   impl Strategy {
       pub fn apply(&mut self, diff: &StrategyDiff) {
           for (tool, weight) in &diff.tool_preferences {
               *self.tool_preferences.entry(tool.clone()).or_insert(0) += weight;
           }
           for (key, threshold) in &diff.confidence_thresholds {
               self.confidence_thresholds.insert(key.clone(), *threshold);
           }
           self.delegation_rules.extend(diff.delegation_rules.clone());
           for patch in &diff.prompt_patches {
               self.prompt_overrides
                   .insert(patch.agent.clone(), patch.content.clone());
           }
           self.version += 1;
           self.updated_at = Some(Utc::now());
       }
   }
   ```

3. Implement `crates/crux-improve/src/policy.rs`:

   ```rust
   use crate::improvement::StrategyDiff;
   use thiserror::Error;

   #[derive(Debug, Clone, Error)]
   pub enum StrategyViolation {
       #[error("too many simultaneous changes: {count} (max {max})")]
       TooManyChanges { count: usize, max: usize },
       #[error("strategy violation: {reason}")]
       Custom { reason: String },
   }

   pub trait StrategyPolicy: Send + Sync {
       fn validate_strategy(&self, diff: &StrategyDiff) -> Result<(), StrategyViolation>;
       fn requires_strategy_approval(&self, diff: &StrategyDiff) -> bool;
   }

   #[derive(Debug, Clone)]
   pub struct DefaultStrategyPolicy {
       pub max_simultaneous_changes: usize,
   }

   impl Default for DefaultStrategyPolicy {
       fn default() -> Self {
           Self { max_simultaneous_changes: 10 }
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
           // Prompt changes and delegation rules are high-risk
           !diff.prompt_patches.is_empty() || !diff.delegation_rules.is_empty()
       }
   }
   ```

4. Implement `crates/crux-improve/src/evolution_adapter.rs`:

   ```rust
   //! Adapter: wraps EvolutionPlanner for the improvement protocol.

   pub use crux_planner::evolution::EvolutionPlanner;
   pub use crux_planner::metrics::RunMetrics;

   use crate::improvement::{ImprovementKind, StrategyDiff};
   use crux_runtime::types::harness::HarnessProfile;

   /// Convert an EvolutionPlanner proposal into a StrategyDiff.
   pub fn evolution_to_strategy_diff(
       planner: &EvolutionPlanner,
       profile: &HarnessProfile,
       metrics: &[RunMetrics],
   ) -> StrategyDiff {
       let harness_diff = planner.propose(profile, metrics);
       if harness_diff.has_changes() {
           StrategyDiff {
               harness_diff: Some(harness_diff),
               ..Default::default()
           }
       } else {
           StrategyDiff::default()
       }
   }
   ```

5. Update `crates/crux-improve/src/lib.rs`:

   ```rust
   pub mod comparison;
   pub mod evolution_adapter;
   pub mod improvement;
   pub mod metrics;
   pub mod policy;

   // Re-export crux types that praxis needs
   pub use crux_types::crux_value::Crux;
   pub use crux_types::id::CruxId;
   pub use crux_types::step::{Step, StepKind, StepStatus};
   pub use crux_types::budget::Budget;
   pub use crux_types::error::CruxErr;

   pub use crux_runtime::safety::{SafetyPolicy, SafetyViolation};
   pub use crux_runtime::types::harness::{HarnessDiff, HarnessProfile};

   // Public API
   pub use comparison::{replay_compare, Comparison, Verdict};
   pub use evolution_adapter::{
       evolution_to_strategy_diff, EvolutionPlanner, RunMetrics,
   };
   pub use improvement::{
       DelegationAction, DelegationRule, Improvement, ImprovementKind,
       PromptPatch, Strategy, StrategyDiff,
   };
   pub use metrics::TraceMetrics;
   pub use policy::{DefaultStrategyPolicy, StrategyPolicy, StrategyViolation};
   ```

6. Verify:

   ```
   cargo nextest run -p crux-improve           -> all green
   cargo clippy -p crux-improve -- -D warnings -> zero warnings
   ```

7. Commit: `git commit -m "feat(crux-improve): add improvement types, StrategyPolicy, and evolution adapter"`

---

### Task 2: Scaffold praxis workspace with port traits

**Crate**: `praxis-core`
**File(s)**: `Cargo.toml` (root), `crates/praxis-core/Cargo.toml`,
`crates/praxis-core/src/lib.rs`, `crates/praxis-core/src/evaluator.rs`,
`crates/praxis-core/src/reward.rs`, `crates/praxis-core/src/strategy.rs`,
`crates/praxis-core/src/store.rs`
**Run**: `cargo nextest run -p praxis-core`

1. Write failing test:

   ```rust
   // crates/praxis-core/src/evaluator.rs
   #[cfg(test)]
   mod tests {
       use super::*;
       use crux_improve::CruxId;

       #[test]
       fn evaluation_roundtrips_json() {
           let e = Evaluation {
               trace_id: CruxId::new(),
               agent: "test".into(),
               score: 0.75,
               findings: vec!["good".into()],
               evaluated_at: chrono::Utc::now(),
           };
           let json = serde_json::to_string(&e).unwrap();
           let back: Evaluation = serde_json::from_str(&json).unwrap();
           assert_eq!(back.agent, "test");
       }
   }
   ```

   Run: `cargo nextest run -p praxis-core`
   Expected: FAIL

2. Create root `~/dev/praxis/Cargo.toml`:

   ```toml
   [workspace]
   members = ["crates/*"]
   resolver = "2"

   [workspace.package]
   version = "0.1.0"
   edition = "2024"
   rust-version = "1.85"
   license = "MIT OR Apache-2.0"

   [workspace.dependencies]
   serde = { version = "1", features = ["derive"] }
   serde_json = "1"
   chrono = { version = "0.4", features = ["serde"] }
   thiserror = "2"
   async-trait = "0.1"
   tokio = { version = "1", features = ["full"] }
   crux-improve = { git = "https://github.com/89jobrien/crux", version = "0.2.5" }
   ```

3. Create `crates/praxis-core/Cargo.toml`:

   ```toml
   [package]
   name = "praxis-core"
   version.workspace = true
   edition.workspace = true
   rust-version.workspace = true
   license.workspace = true
   description = "Port traits for the praxis self-improving runtime"

   [dependencies]
   crux-improve = { workspace = true }
   serde = { workspace = true }
   serde_json = { workspace = true }
   chrono = { workspace = true }
   thiserror = { workspace = true }
   async-trait = { workspace = true }

   [dev-dependencies]
   tokio = { workspace = true }
   ```

4. Implement `evaluator.rs` — `Evaluation` struct + `Evaluator` trait:

   ```rust
   use async_trait::async_trait;
   use chrono::{DateTime, Utc};
   use serde::{Deserialize, Serialize};
   use crux_improve::{Crux, CruxId, TraceMetrics};

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Evaluation {
       pub trace_id: CruxId,
       pub agent: String,
       pub score: f32,
       pub findings: Vec<String>,
       pub metrics: TraceMetrics,
       pub evaluated_at: DateTime<Utc>,
   }

   #[derive(Debug, thiserror::Error)]
   pub enum EvaluationError {
       #[error("evaluation failed: {0}")]
       Failed(String),
   }

   #[async_trait]
   pub trait Evaluator: Send + Sync {
       async fn evaluate(
           &self,
           trace: &Crux<serde_json::Value>,
       ) -> Result<Evaluation, EvaluationError>;
   }
   ```

5. Implement `reward.rs`:

   ```rust
   use async_trait::async_trait;
   use chrono::{DateTime, Duration, Utc};
   use serde::{Deserialize, Serialize};
   use crux_improve::CruxId;

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Reward {
       pub trace_id: CruxId,
       pub agent: String,
       pub score: f32,
       pub recorded_at: DateTime<Utc>,
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   #[serde(rename_all = "snake_case")]
   pub enum TrendDirection { Improving, Declining, Stable }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Trend {
       pub agent: String,
       pub direction: TrendDirection,
       pub slope: f32,
       pub sample_count: usize,
   }

   #[derive(Debug, thiserror::Error)]
   pub enum RewardError {
       #[error("reward store error: {0}")]
       Store(String),
   }

   #[async_trait]
   pub trait RewardAccumulator: Send + Sync {
       async fn record(&mut self, trace_id: CruxId, agent: &str, score: f32)
           -> Result<(), RewardError>;
       async fn query(&self, agent: &str, window: Option<Duration>)
           -> Result<Vec<Reward>, RewardError>;
       async fn trend(&self, agent: &str) -> Result<Trend, RewardError>;
   }
   ```

6. Implement `strategy.rs`:

   ```rust
   use async_trait::async_trait;
   use crux_improve::{Improvement, Strategy};
   use crate::evaluator::Evaluation;
   use crate::reward::Trend;

   #[derive(Debug, thiserror::Error)]
   pub enum PlannerError {
       #[error("planner error: {0}")]
       Failed(String),
   }

   #[async_trait]
   pub trait StrategyPlanner: Send + Sync {
       async fn propose(
           &self,
           evaluation: &Evaluation,
           trend: &Trend,
           current: &Strategy,
       ) -> Result<Vec<Improvement>, PlannerError>;
   }
   ```

7. Implement `store.rs`:

   ```rust
   use crux_improve::{Strategy, StrategyDiff};

   pub trait StrategyStore: Send + Sync {
       fn current(&self) -> Strategy;
       fn apply(&mut self, diff: &StrategyDiff) -> Strategy;
       fn history(&self) -> Vec<Strategy>;
       fn rollback(&mut self, version: u64);
   }
   ```

8. Create `lib.rs`:

   ```rust
   pub mod evaluator;
   pub mod reward;
   pub mod store;
   pub mod strategy;

   pub use evaluator::{Evaluation, EvaluationError, Evaluator};
   pub use reward::{Reward, RewardAccumulator, RewardError, Trend, TrendDirection};
   pub use store::StrategyStore;
   pub use strategy::{PlannerError, StrategyPlanner};
   ```

9. Verify:

   ```
   cargo nextest run -p praxis-core    -> all green
   cargo clippy -p praxis-core -- -D warnings  -> zero warnings
   ```

10. Commit: `git commit -m "feat(praxis-core): scaffold workspace with port traits"`

---

### Task 3: praxis-eval — StubEvaluator + DeterministicStrategyPlanner

**Crate**: `praxis-eval`
**File(s)**: `crates/praxis-eval/Cargo.toml`,
`crates/praxis-eval/src/lib.rs`, `crates/praxis-eval/src/stub.rs`,
`crates/praxis-eval/src/deterministic.rs`
**Run**: `cargo nextest run -p praxis-eval`

1. Write failing test:

   ```rust
   // crates/praxis-eval/src/stub.rs
   #[cfg(test)]
   mod tests {
       use super::*;
       use crux_improve::{Crux, CruxId};
       use chrono::Utc;
       use praxis_core::Evaluator;

       #[tokio::test]
       async fn stub_returns_neutral_score() {
           let eval = StubEvaluator;
           let trace = Crux {
               id: CruxId::new(),
               agent: "test".into(),
               value: Ok(serde_json::json!({})),
               steps: vec![],
               children: vec![],
               started_at: Utc::now(),
               finished_at: Some(Utc::now()),
           };
           let result = eval.evaluate(&trace).await.unwrap();
           assert!((result.score - 0.5).abs() < f32::EPSILON);
       }
   }
   ```

   ```rust
   // crates/praxis-eval/src/deterministic.rs
   #[cfg(test)]
   mod tests {
       use super::*;
       use crux_improve::{CruxId, Strategy, TraceMetrics};
       use praxis_core::{Evaluation, StrategyPlanner, Trend, TrendDirection};
       use chrono::Utc;

       #[tokio::test]
       async fn low_score_proposes_improvements() {
           let planner = DeterministicStrategyPlanner::default();
           let metrics = TraceMetrics {
               step_count: 5, success_rate: 0.3, error_count: 3,
               avg_confidence: 0.4, total_duration_ms: 500,
               delegation_count: 0, delegation_depth: 0,
               speculation_count: 0, speculation_hit_count: 0,
               speculation_hit_rate: 0.0, score: 0.3,
           };
           let eval = Evaluation {
               trace_id: CruxId::new(), agent: "test".into(),
               score: 0.3, findings: vec!["failures".into()],
               metrics, evaluated_at: Utc::now(),
           };
           let trend = Trend {
               agent: "test".into(), direction: TrendDirection::Declining,
               slope: -0.05, sample_count: 10,
           };
           let imps = planner.propose(&eval, &trend, &Strategy::default()).await.unwrap();
           assert!(!imps.is_empty());
       }

       #[tokio::test]
       async fn high_score_proposes_nothing() {
           let planner = DeterministicStrategyPlanner::default();
           let metrics = TraceMetrics {
               step_count: 5, success_rate: 0.95, error_count: 0,
               avg_confidence: 0.9, total_duration_ms: 500,
               delegation_count: 0, delegation_depth: 0,
               speculation_count: 0, speculation_hit_count: 0,
               speculation_hit_rate: 0.0, score: 0.95,
           };
           let eval = Evaluation {
               trace_id: CruxId::new(), agent: "test".into(),
               score: 0.95, findings: vec![],
               metrics, evaluated_at: Utc::now(),
           };
           let trend = Trend {
               agent: "test".into(), direction: TrendDirection::Improving,
               slope: 0.02, sample_count: 10,
           };
           let imps = planner.propose(&eval, &trend, &Strategy::default()).await.unwrap();
           assert!(imps.is_empty());
       }
   }
   ```

   Run: `cargo nextest run -p praxis-eval`
   Expected: FAIL

2. Create `crates/praxis-eval/Cargo.toml`:

   ```toml
   [package]
   name = "praxis-eval"
   version.workspace = true
   edition.workspace = true
   rust-version.workspace = true
   license.workspace = true
   description = "Evaluator and planner adapters for praxis"

   [dependencies]
   praxis-core = { path = "../praxis-core" }
   crux-improve = { workspace = true }
   async-trait = { workspace = true }
   chrono = { workspace = true }
   serde_json = { workspace = true }

   [dev-dependencies]
   tokio = { workspace = true }
   ```

3. Implement `stub.rs` and `deterministic.rs` (StubEvaluator returns
   0.5 + metrics; DeterministicStrategyPlanner proposes confidence
   threshold changes when score < threshold and findings present).

4. Create `lib.rs`:

   ```rust
   pub mod deterministic;
   pub mod stub;
   pub use deterministic::DeterministicStrategyPlanner;
   pub use stub::StubEvaluator;
   ```

5. Verify:

   ```
   cargo nextest run -p praxis-eval    -> all green
   cargo clippy -p praxis-eval -- -D warnings  -> zero warnings
   ```

6. Commit: `git commit -m "feat(praxis-eval): add StubEvaluator and DeterministicStrategyPlanner"`

---

### Task 4: praxis-store — InMemoryRewardStore + FileStrategyStore

**Crate**: `praxis-store`
**File(s)**: `crates/praxis-store/Cargo.toml`,
`crates/praxis-store/src/lib.rs`,
`crates/praxis-store/src/reward_memory.rs`,
`crates/praxis-store/src/strategy_file.rs`
**Run**: `cargo nextest run -p praxis-store`

1. Write failing tests for: record/query rewards, trend computation
   (ascending = Improving, unknown = Stable), strategy roundtrip via
   file, rollback, and history.

2. Create `crates/praxis-store/Cargo.toml`:

   ```toml
   [package]
   name = "praxis-store"
   version.workspace = true
   edition.workspace = true
   rust-version.workspace = true
   license.workspace = true
   description = "Storage adapters for praxis"

   [dependencies]
   praxis-core = { path = "../praxis-core" }
   crux-improve = { workspace = true }
   async-trait = { workspace = true }
   chrono = { workspace = true }
   serde = { workspace = true }
   serde_json = { workspace = true }

   [dev-dependencies]
   tokio = { workspace = true }
   tempfile = "3"
   ```

3. Implement `reward_memory.rs` — `InMemoryRewardStore` with linear
   regression trend detection (slope > 0.01 = Improving, < -0.01 =
   Declining, else Stable).

4. Implement `strategy_file.rs` — `FileStrategyStore` persisting
   `Vec<Strategy>` as JSON, with rollback via truncation.

5. Verify:

   ```
   cargo nextest run -p praxis-store    -> all green
   cargo clippy -p praxis-store -- -D warnings  -> zero warnings
   ```

6. Commit: `git commit -m "feat(praxis-store): add InMemoryRewardStore and FileStrategyStore"`

---

### Task 5: praxis — ImprovementLoop orchestrator

**Crate**: `praxis`
**File(s)**: `crates/praxis/Cargo.toml`, `crates/praxis/src/lib.rs`,
`crates/praxis/src/loop_runner.rs`
**Run**: `cargo nextest run -p praxis`

1. Write failing tests: full cycle evaluates + records reward; second
   cycle produces a Comparison; rollback works.

2. Create `crates/praxis/Cargo.toml`:

   ```toml
   [package]
   name = "praxis"
   version.workspace = true
   edition.workspace = true
   rust-version.workspace = true
   license.workspace = true
   description = "Self-improving agent runtime"

   [dependencies]
   praxis-core = { path = "../praxis-core" }
   praxis-eval = { path = "../praxis-eval" }
   praxis-store = { path = "../praxis-store" }
   crux-improve = { workspace = true }
   serde_json = { workspace = true }
   thiserror = { workspace = true }

   [dev-dependencies]
   tokio = { workspace = true }
   chrono = { workspace = true }
   tempfile = "3"
   ```

3. Implement `loop_runner.rs` — `ImprovementLoop` with:
   - `new(evaluator, planner, store, rewards, policy)` — injects all ports
   - `current_strategy()` — delegates to store
   - `run_cycle(trace)` — evaluate -> record reward -> get trend ->
     propose improvements -> validate via `StrategyPolicy` -> apply ->
     compare with prior trace -> return `CycleResult`
   - `rollback(version)` — delegates to store

   `CycleResult` contains: `evaluation`, `improvements`,
   `strategy`, `comparison: Option<Comparison>`.

   The loop validates each improvement through `StrategyPolicy` before
   applying. If `requires_strategy_approval` returns true, the
   improvement is included in the result but not applied (caller
   decides).

4. Create `lib.rs`:

   ```rust
   pub mod loop_runner;
   pub use loop_runner::{CycleResult, ImprovementLoop, LoopError};
   ```

5. Verify:

   ```
   cargo nextest run -p praxis           -> all green
   cargo clippy -p praxis -- -D warnings -> zero warnings
   ```

6. Commit: `git commit -m "feat(praxis): add ImprovementLoop orchestrator with StrategyPolicy gating"`

---

### Task 6: Project scaffolding — CLAUDE.md, Justfile, LICENSE, README

**Crate**: workspace root
**File(s)**: `CLAUDE.md`, `Justfile`, `README.md`, `LICENSE-MIT`,
`LICENSE-APACHE`, `.ctx/HANDOFF.yaml`
**Run**: `just ci`

1. Create `Justfile`:

   ```just
   default:
       @just --list

   ci: fmt-check lint test

   test:
       cargo nextest run

   lint:
       cargo clippy --all-targets -- -D warnings

   fmt:
       cargo fmt --all

   fmt-check:
       cargo fmt --all -- --check

   build:
       cargo build --all-targets
   ```

2. Create `CLAUDE.md` with build commands, workspace structure, and
   architecture overview. Reference the dependency on `crux-improve`
   and the relationship to the crux ecosystem.

3. Create `README.md` with project description, the improvement loop
   concept, and usage example.

4. Create dual license files (MIT + Apache-2.0).

5. Create `.ctx/HANDOFF.yaml` stub.

6. Verify:

   ```
   just ci  -> all green
   ```

7. Commit: `git commit -m "docs(praxis): add CLAUDE.md, Justfile, README, licenses"`
