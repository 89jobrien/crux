/// Pipeline runner — interprets a parsed YAML pipeline against CruxCtx + HandlerRegistry.
use std::sync::Arc;

use cruxai_core::prelude::*;
use serde_json::Value;

use crate::expr::{ExprContext, StepResult};
use crate::registry::HandlerRegistry;
use crate::schema::{BudgetDef, PipelineDef, SpeculateMode, StepDef};

/// Executes parsed pipelines against a handler registry.
pub struct Runner {
    registry: Arc<HandlerRegistry>,
}

impl Runner {
    pub fn new(registry: Arc<HandlerRegistry>) -> Self {
        Self { registry }
    }

    /// Run a pipeline definition with the given input, producing a full Crux trace.
    pub async fn run(&self, pipeline: &PipelineDef, input: Value) -> Crux<Value> {
        let mut ctx = CruxCtx::new(&pipeline.pipeline);

        if let Some(budget_def) = &pipeline.budget {
            ctx.set_budget(budget_from_def(budget_def));
        }

        let result = self.execute_steps(&mut ctx, &pipeline.steps, input).await;
        ctx.finalize(result)
    }

    async fn execute_steps(
        &self,
        ctx: &mut CruxCtx,
        steps: &[StepDef],
        input: Value,
    ) -> Result<Value, CruxErr> {
        let mut expr_ctx = ExprContext::new(input.clone());
        let mut last_output = input;

        for step_def in steps {
            last_output = self
                .execute_step(ctx, step_def, &last_output, &mut expr_ctx)
                .await?;
        }

        Ok(last_output)
    }

    async fn execute_step(
        &self,
        ctx: &mut CruxCtx,
        step_def: &StepDef,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        match step_def {
            StepDef::Step(node) => {
                let handler_name = node.handler.as_deref().unwrap_or(&node.step);
                let handler = self
                    .registry
                    .get_handler(handler_name)
                    .ok_or_else(|| {
                        CruxErr::step_failed(
                            &node.step,
                            format!("handler not found: {handler_name}"),
                        )
                    })?
                    .clone();
                // Merge static step args into the current input under the "args" key.
                let input = if let Some(step_args) = &node.args {
                    let mut merged = current_input.clone();
                    if let Value::Object(ref mut map) = merged {
                        map.insert("args".to_string(), step_args.clone());
                    } else {
                        merged = serde_json::json!({ "args": step_args, "input": current_input });
                    }
                    merged
                } else {
                    current_input.clone()
                };
                let result = ctx
                    .step(&node.step, || {
                        let h = handler.clone();
                        let i = input.clone();
                        async move { h(i).await }
                    })
                    .await?;
                expr_ctx.steps.insert(
                    node.step.clone(),
                    StepResult {
                        output: result.clone(),
                        confidence: 1.0,
                    },
                );
                Ok(result)
            }

            StepDef::Delegate(node) => {
                let step_name = node.name.as_deref().unwrap_or(&node.delegate);
                let agent_runner = self
                    .registry
                    .get_agent(&node.delegate)
                    .ok_or_else(|| {
                        CruxErr::step_failed(
                            step_name,
                            format!("agent not found: {}", node.delegate),
                        )
                    })?
                    .clone();

                let input = current_input.clone();
                let result = agent_runner(input).await;

                // Record the delegation step in parent
                let output = ctx.step(step_name, || async { result }).await?;

                expr_ctx.steps.insert(
                    step_name.to_string(),
                    StepResult {
                        output: output.clone(),
                        confidence: 1.0,
                    },
                );
                Ok(output)
            }

            StepDef::Pipe(node) => {
                let registry = self.registry.clone();
                #[allow(clippy::type_complexity)]
                let stages: Vec<(
                    &str,
                    Box<dyn FnOnce(Value) -> BoxFut<Value> + Send>,
                )> = node
                    .stages
                    .iter()
                    .map(|stage_name| {
                        let handler = registry.get_handler(stage_name).cloned();
                        let name_owned = stage_name.clone();
                        let stage_fn: Box<dyn FnOnce(Value) -> BoxFut<Value> + Send> =
                            Box::new(move |v: Value| {
                                Box::pin(async move {
                                    let h = handler.ok_or_else(|| {
                                        CruxErr::step_failed(&name_owned, "handler not found")
                                    })?;
                                    h(v).await
                                }) as BoxFut<Value>
                            });
                        // SAFETY: stage_name is borrowed from node.stages which lives for the
                        // duration of this function call. We need to leak a &str for the pipe API.
                        // Instead, we'll collect into owned and use a different approach.
                        (stage_name.as_str(), stage_fn)
                    })
                    .collect();

                let input = current_input.clone();
                let result = ctx.pipe(&node.pipe, input, stages).await?;

                expr_ctx.steps.insert(
                    node.pipe.clone(),
                    StepResult {
                        output: result.clone(),
                        confidence: 1.0,
                    },
                );
                Ok(result)
            }

            StepDef::JoinAll(node) => {
                let arms: Vec<(&str, BoxFut<Value>)> = node
                    .arms
                    .iter()
                    .map(|arm_name| {
                        let handler = self.registry.get_handler(arm_name).cloned();
                        let input = current_input.clone();
                        let name_owned = arm_name.clone();
                        let fut: BoxFut<Value> = Box::pin(async move {
                            let h = handler.ok_or_else(|| {
                                CruxErr::step_failed(&name_owned, "handler not found")
                            })?;
                            h(input).await
                        });
                        (arm_name.as_str(), fut)
                    })
                    .collect();

                let results = ctx.join_all(&node.join_all, arms).await?;
                let output = Value::Array(results);

                expr_ctx.steps.insert(
                    node.join_all.clone(),
                    StepResult {
                        output: output.clone(),
                        confidence: 1.0,
                    },
                );
                Ok(output)
            }

            StepDef::RouteOnConfidence(node) => {
                let confidence = expr_ctx
                    .eval_f32(&node.value)
                    .map_err(|e| CruxErr::step_failed(&node.route_on_confidence, e.to_string()))?;

                let routes: Vec<ConfidenceRoute<'_, Value>> = node
                    .routes
                    .iter()
                    .map(|branch| {
                        let range = parse_range(&branch.range);
                        let handler = self.registry.get_handler(&branch.handler).cloned();
                        let input = current_input.clone();
                        let handler_name = branch.handler.clone();
                        let fut: BoxFut<Value> = Box::pin(async move {
                            let h = handler.ok_or_else(|| {
                                CruxErr::step_failed(&handler_name, "handler not found")
                            })?;
                            h(input).await
                        });
                        (range, branch.label.as_str(), fut)
                    })
                    .collect();

                let result = ctx
                    .route_on_confidence(&node.route_on_confidence, confidence, routes)
                    .await?;

                expr_ctx.steps.insert(
                    node.route_on_confidence.clone(),
                    StepResult {
                        output: result.clone(),
                        confidence,
                    },
                );
                Ok(result)
            }

            StepDef::Speculate(node) => {
                let arms: Vec<(&str, BoxFut<Value>)> = node
                    .arms
                    .iter()
                    .map(|arm_name| {
                        let handler = self.registry.get_handler(arm_name).cloned();
                        let input = current_input.clone();
                        let name_owned = arm_name.clone();
                        let fut: BoxFut<Value> = Box::pin(async move {
                            let h = handler.ok_or_else(|| {
                                CruxErr::step_failed(&name_owned, "handler not found")
                            })?;
                            h(input).await
                        });
                        (arm_name.as_str(), fut)
                    })
                    .collect();

                let builder = ctx.speculate(&node.speculate, arms);
                let result = match node.mode {
                    SpeculateMode::PickBest => {
                        builder
                            .pick_best_by(|v: &Value| {
                                v.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0) as f32
                            })
                            .await?
                    }
                    SpeculateMode::FirstOk => builder.first_ok().await?,
                };

                expr_ctx.steps.insert(
                    node.speculate.clone(),
                    StepResult {
                        output: result.clone(),
                        confidence: 1.0,
                    },
                );
                Ok(result)
            }
        }
    }
}

fn budget_from_def(def: &BudgetDef) -> Budget {
    match def {
        BudgetDef::Tokens { tokens } => Budget::tokens(*tokens),
        BudgetDef::Calls { calls } => Budget::calls(*calls),
        BudgetDef::Duration { duration_ms } => {
            Budget::duration(std::time::Duration::from_millis(*duration_ms))
        }
        BudgetDef::CostCents { cost_cents } => Budget::cost_cents(*cost_cents),
    }
}

/// Parse a range string like `[0.0, 0.5)` or `[0.8, 1.0]`.
fn parse_range(s: &str) -> ConfidenceRange {
    let s = s.trim();
    let inclusive_end = s.ends_with(']');
    let inner = &s[1..s.len() - 1];
    let parts: Vec<&str> = inner.split(',').collect();
    let lo: f32 = parts[0].trim().parse().expect("invalid range lower bound");
    let hi: f32 = parts[1].trim().parse().expect("invalid range upper bound");
    if inclusive_end {
        ConfidenceRange::inclusive(lo, hi)
    } else {
        ConfidenceRange::exclusive(lo, hi)
    }
}
