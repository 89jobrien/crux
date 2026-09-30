//! Multi-arm speculative execution with scored winner selection.

/// SpeculationBuilder — run several approaches, pick the best.
///
/// Created by `CruxCtx::speculate(name, arms)`. Arms run sequentially
/// (concurrent execution deferred to when tokio feature stabilizes).
/// Winner is recorded as Ok, losers as Rejected.
use std::future::Future;
use std::pin::Pin;

use chrono::Utc;

use crate::context::InvocationMeter;
use crate::ctx::CruxCtx;
use crate::types::budget::HandlerUsage;
use crate::types::error::CruxErr;
use crate::types::step::{Step, StepKind, StepOrigin, StepStatus};
use crux_domain::plan_result::PlanResult;
use crux_types::emission::Emission;

/// A named speculation arm.
pub struct SpecArm<T> {
    pub name: String,
    pub fut: Pin<Box<dyn Future<Output = Result<T, CruxErr>> + Send>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BranchScore {
    pub quality: f32,
    pub cost: f32,
    pub latency: f32,
}

impl BranchScore {
    pub const fn new(quality: f32, cost: f32, latency: f32) -> Self {
        Self {
            quality,
            cost,
            latency,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreWeights {
    pub quality: f32,
    pub cost: f32,
    pub latency: f32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            quality: 1.0,
            cost: 0.25,
            latency: 0.25,
        }
    }
}
impl CruxCtx {
    /// Start a speculation: run multiple approaches, pick the best.
    #[allow(clippy::type_complexity)]
    pub fn speculate<'a, T>(
        &'a mut self,
        name: &str,
        arms: Vec<(
            &str,
            Pin<Box<dyn Future<Output = Result<T, CruxErr>> + Send>>,
        )>,
    ) -> SpeculationBuilder<'a, T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
    {
        let spec_arms = arms
            .into_iter()
            .map(|(arm_name, fut)| SpecArm {
                name: arm_name.to_string(),
                fut,
            })
            .collect();
        SpeculationBuilder::new(self, name, spec_arms)
    }
}

pub struct SpeculationBuilder<'a, T> {
    ctx: &'a mut CruxCtx,
    name: String,
    arms: Vec<SpecArm<T>>,
}

impl<'a, T> SpeculationBuilder<'a, T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
{
    pub(crate) fn new(ctx: &'a mut CruxCtx, name: &str, arms: Vec<SpecArm<T>>) -> Self {
        Self {
            ctx,
            name: name.to_string(),
            arms,
        }
    }

    /// Run all arms, pick the one with the highest score from `f`.
    /// Winner is Ok, successful losers are Rejected, failed arms are Err.
    pub async fn pick_best_by<F>(self, f: F) -> Result<T, CruxErr>
    where
        F: Fn(&T) -> f32,
    {
        self.pick_best_by_metered(f, |_| None).await
    }

    pub async fn pick_best_scored<F>(self, weights: ScoreWeights, score: F) -> Result<T, CruxErr>
    where
        F: Fn(&T) -> BranchScore,
    {
        self.pick_best_by(|value| {
            let score = score(value);
            score.quality * weights.quality
                - score.cost * weights.cost
                - score.latency * weights.latency
        })
        .await
    }

    /// Run all arms while recording each completed arm before starting the next.
    pub async fn pick_best_by_metered<F, R>(self, f: F, mut report: R) -> Result<T, CruxErr>
    where
        F: Fn(&T) -> f32,
        R: FnMut(&str) -> Option<(HandlerUsage, std::time::Duration)>,
    {
        self.ctx.emit(Emission::SpeculateStart {
            name: self.name.clone(),
            arm_count: self.arms.len(),
        });
        if let Some(result) = self.planned_result()? {
            return Ok(result);
        }
        let (_ordinal, input_hash) = self.ctx.recorder_mut().next_ordinal(&self.name);

        // Run all arms, collect results
        let mut completed: Vec<(String, Result<T, CruxErr>)> = Vec::new();
        for arm in self.arms {
            self.ctx.reserve_invocations(1)?;
            let result = arm.fut.await;
            if let Some((usage, duration)) = report(&arm.name)
                && let Err(mut accounting_error) =
                    self.ctx.record_invocation_usage(&arm.name, usage, duration)
            {
                if let Err(source) = &result {
                    attach_budget_source(&mut accounting_error, source.clone());
                }
                return Err(accounting_error);
            }
            completed.push((arm.name, result));
        }

        // Find best successful index
        let mut best_idx: Option<usize> = None;
        let mut best_score: f32 = f32::NEG_INFINITY;
        for (i, (_, result)) in completed.iter().enumerate() {
            if let Ok(val) = result {
                let score = f(val);
                if score > best_score {
                    best_score = score;
                    best_idx = Some(i);
                }
            }
        }

        let Some(winner_idx) = best_idx else {
            // All failed
            for (arm_name, result) in &completed {
                let error = match result {
                    Err(e) => e.to_string(),
                    Ok(_) => unreachable!(),
                };
                self.ctx.push_step(Step {
                    stable_id: Some(format!("{}::{}", self.name, arm_name)),
                    name: format!("{}::{}", self.name, arm_name),
                    kind: StepKind::Speculation,
                    status: StepStatus::Err,
                    origin: StepOrigin::Live,
                    confidence: 0.0,
                    started_at: Utc::now(),
                    duration_ms: 0,
                    input_hash,
                    content_hash: None,
                    output: None,
                    error: Some(error),
                    cited_reason: None,
                    attempt: 1,
                    events: vec![],
                    event_subscribers: Default::default(),
                    metadata: std::collections::HashMap::new(),
                    findings: vec![],
                });
            }
            return Err(CruxErr::step_failed(
                &self.name,
                "all speculation arms failed",
            ));
        };

        // Record losers first, extract winner
        let mut winner_val: Option<T> = None;
        for (i, (arm_name, result)) in completed.into_iter().enumerate() {
            if i == winner_idx {
                match result {
                    Ok(val) => {
                        self.ctx.push_step(Step {
                            stable_id: Some(format!("{}::{}", self.name, arm_name)),
                            name: format!("{}::{}", self.name, arm_name),
                            kind: StepKind::Speculation,
                            status: StepStatus::Ok,
                            origin: StepOrigin::Live,
                            confidence: best_score,
                            started_at: Utc::now(),
                            duration_ms: 0,
                            input_hash,
                            content_hash: None,
                            output: serde_json::to_value(&val).ok(),
                            error: None,
                            cited_reason: None,
                            attempt: 1,
                            events: vec![],
                            event_subscribers: Default::default(),
                            metadata: std::collections::HashMap::new(),
                            findings: vec![],
                        });
                        winner_val = Some(val);
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        self.ctx.push_step(Step {
                            stable_id: Some(format!("{}::{}", self.name, arm_name)),
                            name: format!("{}::{}", self.name, arm_name),
                            kind: StepKind::Speculation,
                            status: StepStatus::Err,
                            origin: StepOrigin::Live,
                            confidence: best_score,
                            started_at: Utc::now(),
                            duration_ms: 0,
                            input_hash,
                            content_hash: None,
                            output: None,
                            error: Some(err_msg.clone()),
                            cited_reason: None,
                            attempt: 1,
                            events: vec![],
                            event_subscribers: Default::default(),
                            metadata: std::collections::HashMap::new(),
                            findings: vec![],
                        });
                        return Err(CruxErr::step_failed(&arm_name, err_msg));
                    }
                }
            } else {
                let (status, output, error) = match result {
                    Ok(val) => (StepStatus::Rejected, serde_json::to_value(&val).ok(), None),
                    Err(e) => (StepStatus::Err, None, Some(e.to_string())),
                };
                self.ctx.push_step(Step {
                    stable_id: Some(format!("{}::{}", self.name, arm_name)),
                    name: format!("{}::{}", self.name, arm_name),
                    kind: StepKind::Speculation,
                    status,
                    origin: StepOrigin::Live,
                    confidence: 0.0,
                    started_at: Utc::now(),
                    duration_ms: 0,
                    input_hash,
                    content_hash: None,
                    output,
                    error,
                    cited_reason: None,
                    attempt: 1,
                    events: vec![],
                    event_subscribers: Default::default(),
                    metadata: std::collections::HashMap::new(),
                    findings: vec![],
                });
            }
        }

        Ok(winner_val.unwrap())
    }

    /// Run all arms, pick the one with the highest `score` field in the output JSON.
    ///
    /// Scoring rule:
    /// 1. If the serialized output is a JSON object containing a numeric `"score"` field,
    ///    use that value.
    /// 2. Otherwise, fall back to the byte-length of the serialized output so that arms
    ///    with more content win over arms with no score field (avoiding arbitrary first-wins).
    pub async fn pick_best(self) -> Result<T, CruxErr> {
        let spec_name = self.name.clone();
        self.pick_best_by(|val| {
            let json = serde_json::to_value(val).unwrap_or(serde_json::Value::Null);
            if let Some(score) = json.get("score").and_then(|v| v.as_f64()) {
                return score as f32;
            }
            eprintln!(
                "[crux] warning: speculate '{}' arm has no 'score' field, \
                 falling back to output length",
                spec_name
            );
            json.to_string().len() as f32
        })
        .await
    }

    /// Return the first arm that succeeds. Failed arms recorded as Rejected.
    pub async fn first_ok(self) -> Result<T, CruxErr> {
        self.first_ok_metered(|_| None).await
    }

    /// Return the first successful arm, accounting each completed attempt immediately.
    pub async fn first_ok_metered<R>(self, mut report: R) -> Result<T, CruxErr>
    where
        R: FnMut(&str) -> Option<(HandlerUsage, std::time::Duration)>,
    {
        self.ctx.emit(Emission::SpeculateStart {
            name: self.name.clone(),
            arm_count: self.arms.len(),
        });
        if let Some(result) = self.planned_result()? {
            return Ok(result);
        }
        let (_ordinal, input_hash) = self.ctx.recorder_mut().next_ordinal(&self.name);

        let mut last_err = None;
        for arm in self.arms {
            self.ctx.reserve_invocations(1)?;
            let result = arm.fut.await;
            if let Some((usage, duration)) = report(&arm.name)
                && let Err(mut accounting_error) =
                    self.ctx.record_invocation_usage(&arm.name, usage, duration)
            {
                if let Err(source) = &result {
                    attach_budget_source(&mut accounting_error, source.clone());
                }
                return Err(accounting_error);
            }
            match result {
                Ok(val) => {
                    self.ctx.push_step(Step {
                        stable_id: Some(format!("{}::{}", self.name, arm.name)),
                        name: format!("{}::{}", self.name, arm.name),
                        kind: StepKind::Speculation,
                        status: StepStatus::Ok,
                        origin: StepOrigin::Live,
                        confidence: 1.0,
                        started_at: Utc::now(),
                        duration_ms: 0,
                        input_hash,
                        content_hash: None,
                        output: serde_json::to_value(&val).ok(),
                        error: None,
                        cited_reason: None,
                        attempt: 1,
                        events: vec![],
                        event_subscribers: Default::default(),
                        metadata: std::collections::HashMap::new(),
                        findings: vec![],
                    });
                    return Ok(val);
                }
                Err(e) => {
                    self.ctx.push_step(Step {
                        stable_id: Some(format!("{}::{}", self.name, arm.name)),
                        name: format!("{}::{}", self.name, arm.name),
                        kind: StepKind::Speculation,
                        status: StepStatus::Rejected,
                        origin: StepOrigin::Live,
                        confidence: 0.0,
                        started_at: Utc::now(),
                        duration_ms: 0,
                        input_hash,
                        content_hash: None,
                        output: None,
                        error: Some(e.to_string()),
                        cited_reason: None,
                        attempt: 1,
                        events: vec![],
                        event_subscribers: Default::default(),
                        metadata: std::collections::HashMap::new(),
                        findings: vec![],
                    });
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| CruxErr::step_failed(&self.name, "no speculation arms")))
    }

    fn planned_result(&self) -> Result<Option<T>, CruxErr> {
        match self.ctx.plan_action(&self.name) {
            PlanResult::Allow(_) => Ok(None),
            PlanResult::Deny { reason } => Err(CruxErr::Denied {
                step: self.name.clone(),
                reason,
            }),
            PlanResult::Simulate { output } => {
                serde_json::from_value(output).map(Some).map_err(|error| {
                    CruxErr::step_failed(&self.name, format!("planner simulation: {error}"))
                })
            }
        }
    }
}

fn attach_budget_source(error: &mut CruxErr, source: CruxErr) {
    match error {
        CruxErr::UnreportedCost {
            source: error_source,
            ..
        }
        | CruxErr::UsdBudgetExceeded {
            source: error_source,
            ..
        } => *error_source = Some(Box::new(source)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::CruxCtx;
    use crate::types::error::CruxErr;
    use crate::types::step::StepStatus;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn planner_denies_speculation_before_arms_are_polled() {
        use crux_domain::planner::DenyAllPlanner;

        let polls = Arc::new(AtomicUsize::new(0));
        let arm_polls = Arc::clone(&polls);
        let mut ctx = CruxCtx::new("test");
        ctx.set_planner(DenyAllPlanner {
            reason: "no speculation".into(),
        });
        let result = ctx
            .speculate(
                "choose",
                vec![(
                    "arm",
                    Box::pin(async move {
                        arm_polls.fetch_add(1, Ordering::SeqCst);
                        Ok::<_, CruxErr>(1)
                    }),
                )],
            )
            .first_ok()
            .await;

        assert!(matches!(result, Err(CruxErr::Denied { .. })));
        assert_eq!(polls.load(Ordering::SeqCst), 0);
    }

    fn ok_arm<T: Send + 'static>(name: &str, val: T) -> SpecArm<T> {
        SpecArm {
            name: name.to_string(),
            fut: Box::pin(async move { Ok(val) }),
        }
    }

    fn err_arm<T: Send + 'static>(name: &str) -> SpecArm<T> {
        let name = name.to_string();
        SpecArm {
            name: name.clone(),
            fut: Box::pin(async move { Err(CruxErr::step_failed(&name, "forced failure")) }),
        }
    }

    // pick_best tests

    // -- Issue #9: pick_best uses "score" field, falls back to output length ----

    #[tokio::test]
    async fn pick_best_uses_score_field_from_output() {
        // Arms emit a "score" field; pick_best should select the arm with the
        // highest score, not the first arm (first-wins-on-tie is the bug this fixes).
        let mut ctx = CruxCtx::new("test");
        let arms = vec![
            ok_arm("low", serde_json::json!({"score": 0.1, "answer": "low"})),
            ok_arm("high", serde_json::json!({"score": 0.9, "answer": "high"})),
            ok_arm("mid", serde_json::json!({"score": 0.5, "answer": "mid"})),
        ];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let result = builder.pick_best().await.unwrap();
        assert_eq!(
            result["answer"].as_str().unwrap(),
            "high",
            "expected high-scored arm to win"
        );
    }

    #[tokio::test]
    async fn pick_best_falls_back_to_output_length_when_no_score_field() {
        // Arms emit no "score" field; pick_best should fall back to output JSON
        // length so that the "larger" output wins rather than always picking first.
        let mut ctx = CruxCtx::new("test");
        let arms = vec![
            ok_arm("short", serde_json::json!("hi")),
            ok_arm("long", serde_json::json!("this is a longer answer string")),
        ];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let result = builder.pick_best().await.unwrap();
        assert_eq!(
            result.as_str().unwrap(),
            "this is a longer answer string",
            "expected longer output arm to win when no score field"
        );
    }

    #[tokio::test]
    async fn pick_best_selects_highest_score() {
        let mut ctx = CruxCtx::new("test");
        let arms = vec![
            ok_arm("low", serde_json::json!(1)),
            ok_arm("high", serde_json::json!(2)),
            ok_arm("mid", serde_json::json!(3)),
        ];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        // Score by the integer value — "mid" arm with json(3) scores 3.0 but
        // we use the integer to discriminate; score by the array index via
        // a closure that reads the i64 out of the Value.
        let result = builder
            .pick_best_by(|v| v.as_i64().unwrap_or(0) as f32)
            .await
            .unwrap();
        assert_eq!(result.as_i64().unwrap(), 3);
    }

    #[tokio::test]
    async fn multi_metric_scoring_balances_quality_cost_and_latency() {
        let mut ctx = CruxCtx::new("test");
        let arms = vec![ok_arm("expensive", 1_i32), ok_arm("efficient", 2_i32)];
        let winner = SpeculationBuilder::new(&mut ctx, "rank", arms)
            .pick_best_scored(ScoreWeights::default(), |value| {
                if *value == 1 {
                    BranchScore::new(1.0, 1.0, 1.0)
                } else {
                    BranchScore::new(0.9, 0.1, 0.1)
                }
            })
            .await
            .unwrap();

        assert_eq!(winner, 2);
    }

    #[tokio::test]
    async fn pick_best_all_fail_returns_err() {
        let mut ctx = CruxCtx::new("test");
        let arms: Vec<SpecArm<serde_json::Value>> = vec![err_arm("a"), err_arm("b"), err_arm("c")];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let result = builder.pick_best_by(|_| 1.0).await;
        assert!(result.is_err(), "expected Err when all arms fail");
    }

    #[tokio::test]
    async fn pick_best_all_fail_records_err_steps() {
        let mut ctx = CruxCtx::new("test");
        let arms: Vec<SpecArm<serde_json::Value>> = vec![err_arm("x"), err_arm("y")];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let _ = builder.pick_best_by(|_| 1.0).await;

        let crux = ctx.finalize::<()>(Ok(()));
        assert_eq!(crux.steps.len(), 2);
        assert!(crux.steps.iter().all(|s| s.status == StepStatus::Err));
    }

    #[tokio::test]
    async fn pick_best_winner_ok_losers_rejected() {
        let mut ctx = CruxCtx::new("test");
        let arms = vec![
            ok_arm("winner", serde_json::json!(10)),
            ok_arm("loser", serde_json::json!(1)),
        ];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let _ = builder
            .pick_best_by(|v| v.as_i64().unwrap_or(0) as f32)
            .await
            .unwrap();

        let crux = ctx.finalize::<()>(Ok(()));
        assert_eq!(crux.steps.len(), 2);
        let winner = crux.steps.iter().find(|s| s.status == StepStatus::Ok);
        let loser = crux.steps.iter().find(|s| s.status == StepStatus::Rejected);
        assert!(winner.is_some(), "expected one Ok step");
        assert!(loser.is_some(), "expected one Rejected step");
    }

    #[tokio::test]
    async fn pick_best_tie_break_favors_first_arm() {
        // When two arms have the same score, the first one encountered wins.
        let mut ctx = CruxCtx::new("test");
        let arms = vec![
            ok_arm("first", serde_json::json!("a")),
            ok_arm("second", serde_json::json!("b")),
        ];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        // Both arms score 5.0; first arm should win (strict > comparison).
        let result = builder.pick_best_by(|_| 5.0).await.unwrap();
        assert_eq!(result.as_str().unwrap(), "a");
    }

    #[tokio::test]
    async fn pick_best_multiple_arms_without_score_resolve_deterministically() {
        // Issue #68: arms with no "score" field must not silently tie at 0.0
        // and win arbitrarily by iteration order. The fallback (byte-length of
        // serialized output) must deterministically pick the same winner
        // regardless of arm order.
        let mut ctx_a = CruxCtx::new("test");
        let arms_a = vec![
            ok_arm("first", serde_json::json!({"answer": "short"})),
            ok_arm(
                "second",
                serde_json::json!({"answer": "a much longer answer"}),
            ),
            ok_arm("third", serde_json::json!({"answer": "mid-length"})),
        ];
        let result_a = SpeculationBuilder::new(&mut ctx_a, "spec", arms_a)
            .pick_best()
            .await
            .unwrap();

        let mut ctx_b = CruxCtx::new("test");
        let arms_b = vec![
            ok_arm("third", serde_json::json!({"answer": "mid-length"})),
            ok_arm(
                "second",
                serde_json::json!({"answer": "a much longer answer"}),
            ),
            ok_arm("first", serde_json::json!({"answer": "short"})),
        ];
        let result_b = SpeculationBuilder::new(&mut ctx_b, "spec", arms_b)
            .pick_best()
            .await
            .unwrap();

        // Same logical winner regardless of arm order -- not an arbitrary
        // first-wins tie at score 0.0.
        assert_eq!(result_a["answer"], result_b["answer"]);
        assert_eq!(
            result_a["answer"].as_str().unwrap(),
            "a much longer answer",
            "expected the longest-serialized arm to win deterministically"
        );
    }

    // first_ok tests

    #[tokio::test]
    async fn first_ok_returns_first_success() {
        let mut ctx = CruxCtx::new("test");
        let arms = vec![
            err_arm("fail1"),
            ok_arm("pass", serde_json::json!("found")),
            ok_arm("never", serde_json::json!("skipped")),
        ];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let result = builder.first_ok().await.unwrap();
        assert_eq!(result.as_str().unwrap(), "found");
    }

    #[tokio::test]
    async fn first_ok_all_fail_returns_last_err() {
        let mut ctx = CruxCtx::new("test");
        let arms: Vec<SpecArm<serde_json::Value>> = vec![err_arm("a"), err_arm("b")];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let result = builder.first_ok().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn first_ok_empty_arms_returns_err() {
        let mut ctx = CruxCtx::new("test");
        let arms: Vec<SpecArm<serde_json::Value>> = vec![];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let result = builder.first_ok().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn first_ok_records_failed_as_rejected() {
        let mut ctx = CruxCtx::new("test");
        let arms = vec![err_arm("fail1"), ok_arm("pass", serde_json::json!(true))];
        let builder = SpeculationBuilder::new(&mut ctx, "spec", arms);
        let _ = builder.first_ok().await.unwrap();

        let crux = ctx.finalize::<()>(Ok(()));
        assert_eq!(crux.steps.len(), 2);
        let rejected = crux.steps.iter().find(|s| s.status == StepStatus::Rejected);
        assert!(rejected.is_some());
        let ok = crux.steps.iter().find(|s| s.status == StepStatus::Ok);
        assert!(ok.is_some());
    }
}
