/// Pipeline runner — interprets a parsed YAML pipeline against CruxCtx + HandlerRegistry.
use std::sync::{Arc, Mutex};

use cruxx_core::prelude::*;
use serde_json::Value;

use crate::expr::{ExprContext, ExprError, StepResult};
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
                // Template strings (`{{ input.field }}`, `{{ steps.X.output.field }}`) in
                // args string values are expanded against the current ExprContext before merge.
                let input = if let Some(step_args) = &node.args {
                    let expanded = expand_args(step_args.clone(), expr_ctx);
                    let mut merged = current_input.clone();
                    if let Value::Object(ref mut map) = merged {
                        map.insert("args".to_string(), expanded);
                    } else {
                        merged = serde_json::json!({ "args": expanded, "input": current_input });
                    }
                    merged
                } else {
                    current_input.clone()
                };
                // Invoke the handler once, capturing both value and confidence.
                // Preserve `None` so that `route_on_confidence` can distinguish "no score"
                // (handler_value steps) from an explicit 1.0 — see ExprError::NoConfidence.
                let raw = handler(input.clone()).await?;
                let confidence = raw.confidence;
                let value = raw.value;

                // Record the step in the trace (closure returns the pre-computed value).
                let handler_out = ctx
                    .step(&node.step, || {
                        let v = value.clone();
                        async move { Ok::<Value, CruxErr>(v) }
                    })
                    .await?;

                expr_ctx.steps.insert(
                    node.step.clone(),
                    StepResult {
                        output: handler_out.clone(),
                        confidence,
                    },
                );
                Ok(handler_out)
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
                        confidence: None,
                    },
                );
                Ok(output)
            }

            StepDef::Pipe(node) => {
                let registry = self.registry.clone();

                // One confidence cell per stage; the last stage's confidence wins.
                let confidence_cells: Vec<Arc<Mutex<Option<f32>>>> = node
                    .stages
                    .iter()
                    .map(|_| Arc::new(Mutex::new(None)))
                    .collect();

                #[allow(clippy::type_complexity)]
                let stages: Vec<(
                    &str,
                    Box<dyn FnOnce(Value) -> BoxFut<Value> + Send>,
                )> = node
                    .stages
                    .iter()
                    .zip(confidence_cells.iter())
                    .map(|(arm, cell)| {
                        let handler = registry.get_handler(arm.handler_name()).cloned();
                        let name_owned = arm.handler_name().to_string();
                        let static_args = arm.args().cloned();
                        let cell = Arc::clone(cell);
                        let stage_fn: Box<dyn FnOnce(Value) -> BoxFut<Value> + Send> =
                            Box::new(move |v: Value| {
                                Box::pin(async move {
                                    let h = handler.ok_or_else(|| {
                                        CruxErr::step_failed(&name_owned, "handler not found")
                                    })?;
                                    let input = merge_args(v, static_args);
                                    let out = h(input).await?;
                                    *cell.lock().unwrap() = out.confidence;
                                    Ok(out.value)
                                }) as BoxFut<Value>
                            });
                        (arm.label(), stage_fn)
                    })
                    .collect();

                let input = current_input.clone();
                let result = ctx.pipe(&node.pipe, input, stages).await?;

                // Use the last stage's confidence (pipeline is sequential).
                // Empty stages vec → last() returns None → confidence is None (correct for degenerate case).
                let confidence = confidence_cells.last().and_then(|c| *c.lock().unwrap());

                expr_ctx.steps.insert(
                    node.pipe.clone(),
                    StepResult {
                        output: result.clone(),
                        confidence,
                    },
                );
                Ok(result)
            }

            StepDef::JoinAll(node) => {
                let confidence_cells: Vec<Arc<Mutex<Option<f32>>>> = node
                    .arms
                    .iter()
                    .map(|_| Arc::new(Mutex::new(None)))
                    .collect();

                let arms: Vec<(&str, BoxFut<Value>)> = node
                    .arms
                    .iter()
                    .zip(confidence_cells.iter())
                    .map(|(arm, cell)| {
                        let handler = self.registry.get_handler(arm.handler_name()).cloned();
                        let input = merge_args(current_input.clone(), arm.args().cloned());
                        let name_owned = arm.handler_name().to_string();
                        let cell = Arc::clone(cell);
                        let fut: BoxFut<Value> = Box::pin(async move {
                            let h = handler.ok_or_else(|| {
                                CruxErr::step_failed(&name_owned, "handler not found")
                            })?;
                            let out = h(input).await?;
                            *cell.lock().unwrap() = out.confidence;
                            Ok(out.value)
                        });
                        (arm.label(), fut)
                    })
                    .collect();

                let results = ctx.join_all(&node.join_all, arms).await?;
                let output = Value::Array(results);

                // Average confidence across arms that provided a score; None if none did.
                let scored: Vec<f32> = confidence_cells
                    .iter()
                    .filter_map(|c| *c.lock().unwrap())
                    .collect();
                let confidence = if scored.is_empty() {
                    None
                } else {
                    Some(scored.iter().sum::<f32>() / scored.len() as f32)
                };

                expr_ctx.steps.insert(
                    node.join_all.clone(),
                    StepResult {
                        output: output.clone(),
                        confidence,
                    },
                );
                Ok(output)
            }

            StepDef::RouteOnConfidence(node) => {
                let confidence = expr_ctx
                    .eval_f32(&node.value)
                    .map_err(|e| CruxErr::step_failed(&node.route_on_confidence, e.to_string()))?;

                // One cell per route; only the matching branch's handler will write to it.
                let confidence_cells: Vec<Arc<Mutex<Option<f32>>>> = node
                    .routes
                    .iter()
                    .map(|_| Arc::new(Mutex::new(None)))
                    .collect();

                let routes: Vec<ConfidenceRoute<'_, Value>> = node
                    .routes
                    .iter()
                    .zip(confidence_cells.iter())
                    .map(|(branch, cell)| {
                        let range = parse_range(&branch.range);
                        let handler = self.registry.get_handler(&branch.handler).cloned();
                        let input = merge_args(current_input.clone(), branch.args.clone());
                        let handler_name = branch.handler.clone();
                        let cell = Arc::clone(cell);
                        let fut: BoxFut<Value> = Box::pin(async move {
                            let h = handler.ok_or_else(|| {
                                CruxErr::step_failed(&handler_name, "handler not found")
                            })?;
                            let out = h(input).await?;
                            *cell.lock().unwrap() = out.confidence;
                            Ok(out.value)
                        });
                        (range, branch.label.as_str(), fut)
                    })
                    .collect();

                let result = ctx
                    .route_on_confidence(&node.route_on_confidence, confidence, routes)
                    .await?;

                // Use the matched branch's handler confidence; fall back to the routing score.
                let handler_confidence = confidence_cells
                    .iter()
                    .find_map(|c| *c.lock().unwrap())
                    .map(Some)
                    .unwrap_or(Some(confidence));

                expr_ctx.steps.insert(
                    node.route_on_confidence.clone(),
                    StepResult {
                        output: result.clone(),
                        confidence: handler_confidence,
                    },
                );
                Ok(result)
            }

            StepDef::Speculate(node) => {
                let arms: Vec<(&str, BoxFut<Value>)> = node
                    .arms
                    .iter()
                    .map(|arm| {
                        let handler = self.registry.get_handler(arm.handler_name()).cloned();
                        let input = merge_args(current_input.clone(), arm.args().cloned());
                        let name_owned = arm.handler_name().to_string();
                        let fut: BoxFut<Value> = Box::pin(async move {
                            let h = handler.ok_or_else(|| {
                                CruxErr::step_failed(&name_owned, "handler not found")
                            })?;
                            h(input).await.map(|o| o.value)
                        });
                        (arm.label(), fut)
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
                        confidence: None,
                    },
                );
                Ok(result)
            }
        }
    }
}

/// Recursively expand `{{ expr }}` templates in all string leaves of a JSON value.
///
/// Non-string leaves (numbers, booleans, null, arrays, objects) are traversed but not
/// substituted. Strings that are not `{{ ... }}` templates are returned unchanged.
/// Expansion errors (unknown step, unknown path) are silently ignored — the original
/// string is preserved. This keeps static pipelines working without any ExprContext setup.
fn expand_args(value: Value, ctx: &ExprContext) -> Value {
    match value {
        Value::String(s) => match ctx.eval(&s) {
            Ok(expanded) => expanded,
            Err(ExprError::Syntax(_) | ExprError::UnknownStep(_) | ExprError::UnknownPath(_)) => {
                Value::String(s)
            }
            Err(_) => Value::String(s),
        },
        Value::Array(arr) => Value::Array(arr.into_iter().map(|v| expand_args(v, ctx)).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, expand_args(v, ctx)))
                .collect(),
        ),
        other => other,
    }
}

/// Merge static step args into handler input under the "args" key.
fn merge_args(mut input: Value, args: Option<Value>) -> Value {
    if let Some(a) = args {
        if let Value::Object(ref mut map) = input {
            map.insert("args".to_string(), a);
        } else {
            input = serde_json::json!({ "args": a, "input": input });
        }
    }
    input
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
