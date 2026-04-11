/// SpeculationBuilder — run several approaches, pick the best.
///
/// Created by `CruxCtx::speculate(name, arms)`. Arms run sequentially
/// (concurrent execution deferred to when tokio feature stabilizes).
/// Winner is recorded as Ok, losers as Rejected.
use std::future::Future;
use std::pin::Pin;

use chrono::Utc;

use crate::ctx::CruxCtx;
use crate::types::error::CruxErr;
use crate::types::step::{Step, StepKind, StepStatus};

/// A named speculation arm.
pub struct SpecArm<T> {
    pub name: String,
    pub fut: Pin<Box<dyn Future<Output = Result<T, CruxErr>> + Send>>,
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
        let (_ordinal, input_hash) = self.ctx.recorder_mut().next_ordinal(&self.name);

        // Run all arms, collect results
        let mut completed: Vec<(String, Result<T, CruxErr>)> = Vec::new();
        for arm in self.arms {
            let result = arm.fut.await;
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
                    name: format!("{}::{}", self.name, arm_name),
                    kind: StepKind::Speculation,
                    status: StepStatus::Err,
                    confidence: 0.0,
                    started_at: Utc::now(),
                    duration_ms: 0,
                    input_hash,
                    output: None,
                    error: Some(error),
                    attempt: 1,
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
                let val = result.unwrap();
                // Record the winner step
                self.ctx.push_step(Step {
                    name: format!("{}::{}", self.name, arm_name),
                    kind: StepKind::Speculation,
                    status: StepStatus::Ok,
                    confidence: best_score,
                    started_at: Utc::now(),
                    duration_ms: 0,
                    input_hash,
                    output: serde_json::to_value(&val).ok(),
                    error: None,
                    attempt: 1,
                });
                winner_val = Some(val);
            } else {
                let (status, output, error) = match result {
                    Ok(val) => (StepStatus::Rejected, serde_json::to_value(&val).ok(), None),
                    Err(e) => (StepStatus::Err, None, Some(e.to_string())),
                };
                self.ctx.push_step(Step {
                    name: format!("{}::{}", self.name, arm_name),
                    kind: StepKind::Speculation,
                    status,
                    confidence: 0.0,
                    started_at: Utc::now(),
                    duration_ms: 0,
                    input_hash,
                    output,
                    error,
                    attempt: 1,
                });
            }
        }

        Ok(winner_val.unwrap())
    }

    /// Return the first arm that succeeds. Failed arms recorded as Rejected.
    pub async fn first_ok(self) -> Result<T, CruxErr> {
        let (_ordinal, input_hash) = self.ctx.recorder_mut().next_ordinal(&self.name);

        let mut last_err = None;
        for arm in self.arms {
            match arm.fut.await {
                Ok(val) => {
                    self.ctx.push_step(Step {
                        name: format!("{}::{}", self.name, arm.name),
                        kind: StepKind::Speculation,
                        status: StepStatus::Ok,
                        confidence: 1.0,
                        started_at: Utc::now(),
                        duration_ms: 0,
                        input_hash,
                        output: serde_json::to_value(&val).ok(),
                        error: None,
                        attempt: 1,
                    });
                    return Ok(val);
                }
                Err(e) => {
                    self.ctx.push_step(Step {
                        name: format!("{}::{}", self.name, arm.name),
                        kind: StepKind::Speculation,
                        status: StepStatus::Rejected,
                        confidence: 0.0,
                        started_at: Utc::now(),
                        duration_ms: 0,
                        input_hash,
                        output: None,
                        error: Some(e.to_string()),
                        attempt: 1,
                    });
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| CruxErr::step_failed(&self.name, "no speculation arms")))
    }
}
