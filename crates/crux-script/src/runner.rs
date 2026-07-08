/// Pipeline runner — interprets a parsed YAML pipeline against CruxCtx + HandlerRegistry.
use std::sync::{Arc, Mutex};

use crux_runtime::prelude::*;
use serde_json::Value;

use crate::expr::{ExprContext, ExprError, StepResult};
use crate::registry::HandlerRegistry;
use crate::schema::{
    BudgetDef, DelegateNode, JoinAllNode, PipeNode, PipelineDef, RouteNode, SpeculateMode,
    SpeculateNode, StepDef, StepNode, TargetDef,
};

/// Executes parsed pipelines against a handler registry.
pub struct Runner {
    registry: Arc<HandlerRegistry>,
}

impl Runner {
    pub fn new(registry: Arc<HandlerRegistry>) -> Self {
        Self { registry }
    }

    /// Run a pipeline definition with the given input, producing a full Crux trace.
    ///
    /// Validates the pipeline against the handler registry before execution.
    /// If validation produces any errors, returns immediately with a failed trace.
    /// Warnings are printed to stderr.
    pub async fn run(&self, pipeline: &PipelineDef, input: Value) -> Crux<Value> {
        if let Some(crux) = self.validate_or_fail(pipeline) {
            return crux;
        }
        self.run_core(pipeline, input, None, ReplayMode::Strict)
            .await
    }

    /// Run a pipeline with replay from a previous trace.
    ///
    /// Steps whose name + input hash match the previous trace are served from
    /// cache instead of re-executing. `mode` controls matching strictness.
    pub async fn run_with_replay(
        &self,
        pipeline: &PipelineDef,
        input: Value,
        previous: &Crux<Value>,
        mode: ReplayMode,
    ) -> Crux<Value> {
        if let Some(crux) = self.validate_or_fail(pipeline) {
            return crux;
        }
        self.run_core(pipeline, input, Some(previous), mode).await
    }

    /// Run without pre-flight validation. Use for tests or REPL.
    pub async fn run_unchecked(&self, pipeline: &PipelineDef, input: Value) -> Crux<Value> {
        self.run_core(pipeline, input, None, ReplayMode::Strict)
            .await
    }

    /// Validate and return a failed Crux if errors exist, or None to proceed.
    fn validate_or_fail(&self, pipeline: &PipelineDef) -> Option<Crux<Value>> {
        let report = crate::validator::validate_pipeline(pipeline, &self.registry);
        for diag in &report.diagnostics {
            if diag.severity == crate::validator::DiagnosticSeverity::Warning {
                eprintln!("[crux] warning: {}: {}", diag.location, diag.message);
            }
        }
        if !report.is_ok() {
            let errors: Vec<String> = report
                .diagnostics
                .iter()
                .filter(|d| d.severity == crate::validator::DiagnosticSeverity::Error)
                .map(|d| format!("{}: {}", d.location, d.message))
                .collect();
            let ctx = CruxCtx::new(&pipeline.pipeline);
            let err = CruxErr::step_failed(
                &pipeline.pipeline,
                format!("pipeline validation failed:\n{}", errors.join("\n")),
            );
            return Some(ctx.finalize(Err(err)));
        }
        None
    }

    /// Run a single Cruxfile target with the given name and optional budget override.
    pub async fn run_target(
        &self,
        target: &TargetDef,
        name: &str,
        budget_override: Option<&BudgetDef>,
    ) -> Crux<Value> {
        let mut ctx = CruxCtx::new(name);

        if let Some(budget_def) = budget_override.or(target.budget.as_ref()) {
            ctx.set_budget(budget_from_def(budget_def));
        }

        let result = self
            .execute_steps(&mut ctx, &target.steps, Value::Null)
            .await;
        ctx.finalize(result)
    }

    async fn run_core(
        &self,
        pipeline: &PipelineDef,
        input: Value,
        previous: Option<&Crux<Value>>,
        mode: ReplayMode,
    ) -> Crux<Value> {
        let mut ctx = CruxCtx::new(&pipeline.pipeline);

        if let Some(budget_def) = &pipeline.budget {
            ctx.set_budget(budget_from_def(budget_def));
        }

        if let Some(prev) = previous {
            ctx.set_replay_mode(mode);
            ctx.replay_from(prev);
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
                self.execute_handler_step(ctx, node, current_input, expr_ctx)
                    .await
            }
            StepDef::Delegate(node) => {
                self.execute_delegate_step(ctx, node, current_input, expr_ctx)
                    .await
            }
            StepDef::Pipe(node) => {
                self.execute_pipe_step(ctx, node, current_input, expr_ctx)
                    .await
            }
            StepDef::JoinAll(node) => {
                self.execute_join_all_step(ctx, node, current_input, expr_ctx)
                    .await
            }
            StepDef::RouteOnConfidence(node) => {
                self.execute_route_on_confidence_step(ctx, node, current_input, expr_ctx)
                    .await
            }
            StepDef::Speculate(node) => {
                self.execute_speculate_step(ctx, node, current_input, expr_ctx)
                    .await
            }
        }
    }

    /// Execute a `step:` node — resolves the handler, expands args, runs via `ctx.step()`.
    async fn execute_handler_step(
        &self,
        ctx: &mut CruxCtx,
        node: &StepNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let handler_name = node.handler.as_deref().unwrap_or(&node.step);
        let handler = self
            .registry
            .get_handler(handler_name)
            .ok_or_else(|| {
                CruxErr::step_failed(&node.step, format!("handler not found: {handler_name}"))
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

        // Run the handler inside ctx.step() so replay can skip it entirely.
        // Confidence is captured via a shared cell since ctx.step() only returns the value.
        let confidence_cell: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));
        let cc = confidence_cell.clone();
        let handler_out = ctx
            .step(&node.step, move || async move {
                let raw = handler(input).await?;
                *cc.lock().unwrap() = raw.confidence;
                Ok::<Value, CruxErr>(raw.value)
            })
            .await?;

        let confidence = *confidence_cell.lock().unwrap();
        expr_ctx.steps.insert(
            node.step.clone(),
            StepResult {
                output: handler_out.clone(),
                confidence,
            },
        );
        Ok(handler_out)
    }

    /// Execute a `delegate:` node — looks up a registered agent and runs it via `ctx.step()`.
    async fn execute_delegate_step(
        &self,
        ctx: &mut CruxCtx,
        node: &DelegateNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let step_name = node.name.as_deref().unwrap_or(&node.delegate);
        let agent_runner = self
            .registry
            .get_agent(&node.delegate)
            .ok_or_else(|| {
                CruxErr::step_failed(step_name, format!("agent not found: {}", node.delegate))
            })?
            .clone();

        let input = current_input.clone();
        let result = agent_runner(input).await;

        // Record the delegation step in parent.
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

    /// Execute a `pipe:` node — chains stages sequentially via `ctx.pipe()`.
    async fn execute_pipe_step(
        &self,
        ctx: &mut CruxCtx,
        node: &PipeNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let registry = self.registry.clone();

        // One confidence cell per stage; the last stage's confidence wins.
        let confidence_cells: Vec<Arc<Mutex<Option<f32>>>> = node
            .stages
            .iter()
            .map(|_| Arc::new(Mutex::new(None)))
            .collect();

        #[allow(clippy::type_complexity)]
        let stages: Vec<(&str, Box<dyn FnOnce(Value) -> BoxFut<Value> + Send>)> = node
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

    /// Execute a `join_all:` node — fans out arms concurrently via `ctx.join_all()`.
    async fn execute_join_all_step(
        &self,
        ctx: &mut CruxCtx,
        node: &JoinAllNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
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
                    let h = handler
                        .ok_or_else(|| CruxErr::step_failed(&name_owned, "handler not found"))?;
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

    /// Execute a `route_on_confidence:` node — dispatches to one handler based on a
    /// confidence score evaluated from `expr_ctx`.
    async fn execute_route_on_confidence_step(
        &self,
        ctx: &mut CruxCtx,
        node: &RouteNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
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
                    let h = handler
                        .ok_or_else(|| CruxErr::step_failed(&handler_name, "handler not found"))?;
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

    /// Execute a `speculate:` node — races arms via `ctx.speculate()`.
    async fn execute_speculate_step(
        &self,
        ctx: &mut CruxCtx,
        node: &SpeculateNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let arms: Vec<(&str, BoxFut<Value>)> = node
            .arms
            .iter()
            .map(|arm| {
                let handler = self.registry.get_handler(arm.handler_name()).cloned();
                let input = merge_args(current_input.clone(), arm.args().cloned());
                let name_owned = arm.handler_name().to_string();
                let fut: BoxFut<Value> = Box::pin(async move {
                    let h = handler
                        .ok_or_else(|| CruxErr::step_failed(&name_owned, "handler not found"))?;
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
    let mut budgets = Vec::new();
    if let Some(tokens) = def.tokens {
        budgets.push(Budget::tokens(tokens));
    }
    if let Some(calls) = def.calls {
        budgets.push(Budget::calls(calls));
    }
    if let Some(duration_ms) = def.duration_ms {
        budgets.push(Budget::duration(std::time::Duration::from_millis(
            duration_ms,
        )));
    }
    if let Some(cost_cents) = def.cost_cents {
        budgets.push(Budget::cost_cents(cost_cents));
    }
    match budgets.as_slice() {
        [] => Budget::default(),
        [_] => budgets.into_iter().next().unwrap_or_default(),
        _ => Budget::combined(budgets),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Build a minimal pipeline with a single step that calls a counting handler.
    fn counting_pipeline() -> (PipelineDef, HandlerRegistry, Arc<AtomicU32>) {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let mut reg = HandlerRegistry::new();
        reg.handler_value("test::count", move |input: Value| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(input)
            }
        });

        let pipeline = crate::load(
            "pipeline: replay_test\nsteps:\n  - step: count_step\n    handler: test::count\n",
        )
        .expect("valid pipeline");

        (pipeline, reg, counter)
    }

    #[tokio::test]
    async fn run_without_replay_executes_handler() {
        let (pipeline, reg, counter) = counting_pipeline();
        let runner = Runner::new(Arc::new(reg));

        let crux = runner.run(&pipeline, serde_json::json!({})).await;
        assert!(crux.value().is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_with_replay_skips_cached_steps() {
        let (pipeline, reg, counter) = counting_pipeline();
        let runner = Runner::new(Arc::new(reg));

        // First run — handler executes, produces trace.
        let trace = runner.run(&pipeline, serde_json::json!({})).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Second run — replay from first trace, handler should NOT execute again.
        let replayed = runner
            .run_with_replay(&pipeline, serde_json::json!({}), &trace, ReplayMode::Strict)
            .await;
        assert!(replayed.value().is_ok());
        // Counter stays at 1 — the handler was not called during replay.
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "handler should not re-execute during replay"
        );
    }

    #[tokio::test]
    async fn replay_strict_errors_on_step_name_mismatch() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let mut reg = HandlerRegistry::new();
        reg.handler_value("test::count", move |input: Value| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(input)
            }
        });

        // First pipeline has step named "alpha".
        let pipeline_a = crate::load(
            "pipeline: replay_test\nsteps:\n  - step: alpha\n    handler: test::count\n",
        )
        .expect("valid pipeline");

        // Second pipeline has step named "beta" at the same ordinal.
        let pipeline_b = crate::load(
            "pipeline: replay_test\nsteps:\n  - step: beta\n    handler: test::count\n",
        )
        .expect("valid pipeline");

        let runner = Runner::new(Arc::new(reg));
        let trace = runner.run(&pipeline_a, serde_json::json!({})).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Replay pipeline_b against pipeline_a's trace — name mismatch at ordinal 0.
        let replayed = runner
            .run_with_replay(
                &pipeline_b,
                serde_json::json!({}),
                &trace,
                ReplayMode::Strict,
            )
            .await;
        assert!(
            replayed.value().is_err(),
            "strict replay with different step name should fail with mismatch"
        );
    }

    #[tokio::test]
    async fn replay_lenient_reexecutes_on_step_name_mismatch() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let mut reg = HandlerRegistry::new();
        reg.handler_value("test::count", move |input: Value| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(input)
            }
        });

        let pipeline_a = crate::load(
            "pipeline: replay_test\nsteps:\n  - step: alpha\n    handler: test::count\n",
        )
        .expect("valid pipeline");

        let pipeline_b = crate::load(
            "pipeline: replay_test\nsteps:\n  - step: beta\n    handler: test::count\n",
        )
        .expect("valid pipeline");

        let runner = Runner::new(Arc::new(reg));
        let trace = runner.run(&pipeline_a, serde_json::json!({})).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Lenient mode: name mismatch returns Miss, so handler re-executes.
        let replayed = runner
            .run_with_replay(
                &pipeline_b,
                serde_json::json!({}),
                &trace,
                ReplayMode::Lenient,
            )
            .await;
        assert!(replayed.value().is_ok());
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "lenient replay should re-execute on step name mismatch"
        );
    }

    #[tokio::test]
    async fn trace_roundtrips_through_json() {
        let (pipeline, reg, _) = counting_pipeline();
        let runner = Runner::new(Arc::new(reg));

        let trace = runner.run(&pipeline, serde_json::json!({"x": 1})).await;

        // Serialize and deserialize — simulates --save-trace / --replay.
        let json = serde_json::to_string(&trace).expect("serialize trace");
        let restored: Crux<Value> = serde_json::from_str(&json).expect("deserialize trace");

        assert_eq!(trace.steps.len(), restored.steps.len());
        assert_eq!(trace.steps[0].name, restored.steps[0].name);
        assert_eq!(trace.steps[0].input_hash, restored.steps[0].input_hash);
    }

    #[tokio::test]
    async fn run_rejects_pipeline_with_validation_errors() {
        let yaml = r#"
pipeline: bad
steps:
  - step: s1
    handler: shell::nonexistent
"#;
        let pipeline = crate::load(yaml).unwrap();
        let mut reg = HandlerRegistry::new();
        // Register one shell:: handler so the namespace is known.
        reg.handler_value("shell::exec", |v: Value| async { Ok(v) });
        let runner = Runner::new(Arc::new(reg));
        let crux = runner.run(&pipeline, serde_json::json!({})).await;
        assert!(crux.value().is_err());
        let err = crux.value().unwrap_err();
        assert!(
            err.to_string().contains("validation"),
            "expected validation error, got: {err}"
        );
    }

    #[tokio::test]
    async fn run_unchecked_skips_validation() {
        let yaml = r#"
pipeline: bad
steps:
  - step: s1
    handler: shell::nonexistent
"#;
        let pipeline = crate::load(yaml).unwrap();
        let reg = HandlerRegistry::new();
        let runner = Runner::new(Arc::new(reg));
        // run_unchecked should attempt execution (and fail at handler lookup, not validation)
        let crux = runner.run_unchecked(&pipeline, serde_json::json!({})).await;
        assert!(crux.value().is_err());
        let err = crux.value().unwrap_err();
        assert!(
            !err.to_string().contains("validation"),
            "unchecked should not validate, got: {err}"
        );
    }
}
