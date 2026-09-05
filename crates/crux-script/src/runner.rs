/// Pipeline runner — interprets a parsed YAML pipeline against CruxCtx + HandlerRegistry.
use std::sync::{Arc, Mutex};

use crux_runtime::prelude::*;
use indexmap::IndexMap;
use serde_json::Value;

use crate::expr::IterFrame;
use crate::expr::{ExprContext, ExprError, StepResult};
use crate::registry::HandlerRegistry;
use crate::schema::{
    BudgetDef, DelegateNode, ExpectDef, ForEachNode, JoinAllNode, OnErrorDef, PipeNode,
    PipelineDef, PollNode, RepeatNode, RouteNode, SpeculateMode, SpeculateNode, StepDef, StepNode,
    TargetDef, WhileNode,
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

        let empty_vars = IndexMap::new();
        let result = self
            .execute_steps(&mut ctx, &target.steps, Value::Null, &empty_vars)
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

        let empty_vars = IndexMap::new();
        let vars = pipeline.vars.as_ref().unwrap_or(&empty_vars);
        let result = self
            .execute_steps(&mut ctx, &pipeline.steps, input, vars)
            .await;
        ctx.finalize(result)
    }

    async fn execute_steps(
        &self,
        ctx: &mut CruxCtx,
        steps: &[StepDef],
        input: Value,
        vars: &IndexMap<String, Value>,
    ) -> Result<Value, CruxErr> {
        let mut expr_ctx = ExprContext::new(input.clone());

        // Resolve vars: (#85) once, up front, in declaration order so a var may
        // reference input.* or an earlier-declared var. Steps see the fully
        // resolved map via `{{ vars.NAME }}`.
        for (name, raw) in vars {
            let resolved = expand_args(raw.clone(), &expr_ctx);
            expr_ctx.vars.insert(name.clone(), resolved);
        }

        self.execute_steps_with_ctx(ctx, steps, input, &mut expr_ctx)
            .await
    }

    /// Run a list of steps against an already-built `ExprContext`, without creating
    /// a fresh one or resolving `vars:`. Used both by [`Self::execute_steps`] (the
    /// top-level pipeline body) and by loop constructs (`poll`, `for_each`, `while`,
    /// `repeat`) that need their nested `steps:` block to share the outer scope's
    /// `vars`/`steps` map while adding per-iteration `iter.*` bindings.
    // Returns a boxed future: this method participates in a recursion cycle
    // (loop constructs call it, which calls execute_step, which calls back into
    // loop-construct executors), and `async fn` cannot recurse without indirection.
    fn execute_steps_with_ctx<'a>(
        &'a self,
        ctx: &'a mut CruxCtx,
        steps: &'a [StepDef],
        input: Value,
        expr_ctx: &'a mut ExprContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, CruxErr>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut last_output = input;

            for step_def in steps {
                last_output = self
                    .execute_step(ctx, step_def, &last_output, expr_ctx)
                    .await?;
            }

            Ok(last_output)
        })
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
            StepDef::Poll(node) => {
                self.execute_poll_step(ctx, node, current_input, expr_ctx)
                    .await
            }
            StepDef::ForEach(node) => {
                self.execute_for_each_step(ctx, node, current_input, expr_ctx)
                    .await
            }
            StepDef::While(node) => {
                self.execute_while_step(ctx, node, current_input, expr_ctx)
                    .await
            }
            StepDef::Repeat(node) => {
                self.execute_repeat_step(ctx, node, current_input, expr_ctx)
                    .await
            }
        }
    }

    /// Execute a `while:` node — pre-condition loop (#89). `condition:` is
    /// checked before each iteration (including the first); the loop runs only
    /// while it's truthy. Each iteration is a traced sub-step named
    /// `<while>[<index>]`; `break_if:` can stop the loop early.
    async fn execute_while_step(
        &self,
        ctx: &mut CruxCtx,
        node: &WhileNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let label = node.r#while.as_str();
        let mut last_output = current_input.clone();
        let mut index: usize = 0;

        loop {
            let cont = expr_ctx
                .eval_bool(&node.condition)
                .map_err(|e| CruxErr::step_failed(label, e.to_string()))?;
            if !cont {
                break;
            }

            let saved_iter = expr_ctx.iter.take();
            expr_ctx.iter = Some(IterFrame {
                index,
                values: std::collections::HashMap::new(),
            });

            let iter_result = self
                .execute_steps_with_ctx(ctx, &node.steps, last_output.clone(), expr_ctx)
                .await;
            let iter_output = match iter_result {
                Ok(v) => v,
                Err(e) => {
                    expr_ctx.iter = saved_iter;
                    return Err(e);
                }
            };

            let iteration_label = format!("{label}[{index}]");
            let marker_value = iter_output.clone();
            if let Err(e) = ctx
                .step(&iteration_label, move || async move {
                    Ok::<Value, CruxErr>(marker_value)
                })
                .await
            {
                expr_ctx.iter = saved_iter;
                return Err(e);
            }

            last_output = iter_output;
            index += 1;

            let should_break = match &node.break_if {
                Some(expr) => expr_ctx
                    .eval_bool(expr)
                    .map_err(|e| CruxErr::step_failed(label, e.to_string()))?,
                None => false,
            };

            expr_ctx.iter = saved_iter;

            if should_break {
                break;
            }
        }

        expr_ctx.steps.insert(
            label.to_string(),
            StepResult {
                output: last_output.clone(),
                confidence: None,
            },
        );
        Ok(last_output)
    }

    /// Execute a `repeat:` node — fixed-count loop (#89). Runs `steps:` exactly
    /// `count` times. Each iteration is a traced sub-step named
    /// `<repeat>[<index>]`; `break_if:` can stop the loop early.
    async fn execute_repeat_step(
        &self,
        ctx: &mut CruxCtx,
        node: &RepeatNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let label = node.repeat.as_str();
        let mut last_output = current_input.clone();

        for index in 0..node.count as usize {
            let saved_iter = expr_ctx.iter.take();
            expr_ctx.iter = Some(IterFrame {
                index,
                values: std::collections::HashMap::new(),
            });

            let iter_result = self
                .execute_steps_with_ctx(ctx, &node.steps, last_output.clone(), expr_ctx)
                .await;
            let iter_output = match iter_result {
                Ok(v) => v,
                Err(e) => {
                    expr_ctx.iter = saved_iter;
                    return Err(e);
                }
            };

            let iteration_label = format!("{label}[{index}]");
            let marker_value = iter_output.clone();
            if let Err(e) = ctx
                .step(&iteration_label, move || async move {
                    Ok::<Value, CruxErr>(marker_value)
                })
                .await
            {
                expr_ctx.iter = saved_iter;
                return Err(e);
            }

            last_output = iter_output;

            let should_break = match &node.break_if {
                Some(expr) => expr_ctx
                    .eval_bool(expr)
                    .map_err(|e| CruxErr::step_failed(label, e.to_string()))?,
                None => false,
            };

            expr_ctx.iter = saved_iter;

            if should_break {
                break;
            }
        }

        expr_ctx.steps.insert(
            label.to_string(),
            StepResult {
                output: last_output.clone(),
                confidence: None,
            },
        );
        Ok(last_output)
    }

    /// Execute a `for_each:` node — maps `steps:` over each item in `items:` (#84).
    ///
    /// `items:` is evaluated once against the outer scope to produce the array.
    /// Each iteration binds `{{ iter.<as> }}` and `{{ iter.index }}`, runs the
    /// nested `steps:` block, and records the iteration as a traced sub-step named
    /// `<for_each>[<index>]`. `break_if:` (evaluated after each iteration) stops
    /// the loop early.
    ///
    /// Iterations run sequentially even when `parallel: true` — `CruxCtx` is a
    /// single mutable trace recorder in this crate's architecture (unlike
    /// `join_all`, whose arms don't touch `ctx` until the runtime's own internal
    /// fan-out), so concurrent nested `ctx.step()` calls across iterations aren't
    /// sound without a `crux-runtime` change, which is out of scope here.
    /// `parallel`/`max_concurrency` are accepted for forward compatibility.
    async fn execute_for_each_step(
        &self,
        ctx: &mut CruxCtx,
        node: &ForEachNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let label = node.label();
        let binding = node.binding();

        let items_value = expr_ctx
            .eval(&node.items)
            .map_err(|e| CruxErr::step_failed(label, e.to_string()))?;
        let items = items_value.as_array().cloned().ok_or_else(|| {
            CruxErr::step_failed(
                label,
                format!("items: did not resolve to an array: {items_value}"),
            )
        })?;

        let mut last_output = current_input.clone();

        for (index, item) in items.iter().enumerate() {
            let saved_iter = expr_ctx.iter.take();
            expr_ctx.iter = Some(IterFrame {
                index,
                values: std::collections::HashMap::from([(binding.to_string(), item.clone())]),
            });

            let iter_result = self
                .execute_steps_with_ctx(ctx, &node.steps, last_output.clone(), expr_ctx)
                .await;

            let iter_output = match iter_result {
                Ok(v) => v,
                Err(e) => {
                    expr_ctx.iter = saved_iter;
                    return Err(e);
                }
            };

            let iteration_label = format!("{label}[{index}]");
            let marker_value = iter_output.clone();
            if let Err(e) = ctx
                .step(&iteration_label, move || async move {
                    Ok::<Value, CruxErr>(marker_value)
                })
                .await
            {
                expr_ctx.iter = saved_iter;
                return Err(e);
            }

            last_output = iter_output;

            let should_break = match &node.break_if {
                Some(expr) => expr_ctx
                    .eval_bool(expr)
                    .map_err(|e| CruxErr::step_failed(label, e.to_string()))?,
                None => false,
            };

            expr_ctx.iter = saved_iter;

            if should_break {
                break;
            }
        }

        expr_ctx.steps.insert(
            label.to_string(),
            StepResult {
                output: last_output.clone(),
                confidence: None,
            },
        );
        Ok(last_output)
    }

    /// Execute a `poll:` node — do-while semantics (#83). Runs `steps:` at least
    /// once, then repeats until `until:` is truthy or `max_attempts` is reached.
    /// Each iteration's output is recorded as a traced sub-step named
    /// `<poll>[<index>]` (0-based) so the trace shows exactly how many attempts ran.
    async fn execute_poll_step(
        &self,
        ctx: &mut CruxCtx,
        node: &PollNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let mut last_output = current_input.clone();
        let mut index: u32 = 0;

        loop {
            let iter_output = self
                .execute_steps_with_ctx(ctx, &node.steps, last_output.clone(), expr_ctx)
                .await?;

            // Record the iteration itself as a traced sub-step (#83). The nested
            // steps above are already individually traced; this marker makes the
            // iteration boundary visible in the trace.
            let label = format!("{}[{}]", node.poll, index);
            let marker_value = iter_output.clone();
            ctx.step(
                &label,
                move || async move { Ok::<Value, CruxErr>(marker_value) },
            )
            .await?;

            last_output = iter_output;
            index += 1;

            let done = expr_ctx
                .eval_bool(&node.until)
                .map_err(|e| CruxErr::step_failed(&node.poll, e.to_string()))?;
            if done {
                break;
            }
            if let Some(max) = node.max_attempts
                && index >= max
            {
                break;
            }
            if let Some(ms) = node.interval_ms {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            }
        }

        expr_ctx.steps.insert(
            node.poll.clone(),
            StepResult {
                output: last_output.clone(),
                confidence: None,
            },
        );
        Ok(last_output)
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

        // Retry-with-backoff (#79): attempt 1 (initial) plus `retry.count` more,
        // each recorded as its own traced sub-step so replay/inspection can see
        // exactly which attempts ran and failed.
        let (max_attempts, delay_ms) = match &node.retry {
            Some(r) => (r.count + 1, r.delay_ms),
            None => (1, 0),
        };

        let mut last_err: Option<CruxErr> = None;
        let mut success: Option<(Value, Option<f32>)> = None;
        for attempt in 0..max_attempts {
            let step_label = if node.retry.is_some() {
                format!("{}::attempt{}", node.step, attempt + 1)
            } else {
                node.step.clone()
            };
            match run_step_once(
                ctx,
                &step_label,
                handler.clone(),
                input.clone(),
                node.timeout_ms,
            )
            .await
            {
                Ok(ok) => {
                    success = Some(ok);
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt + 1 < max_attempts && delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        let (handler_out, confidence) = match success {
            Some(ok) => ok,
            None => {
                let e = last_err.expect("loop runs at least once, so failure implies an error");

                // on_error (#88): active recovery, tried before falling back to
                // allow_failure's passive tolerance.
                if let Some(on_err) = &node.on_error {
                    match self
                        .run_on_error(ctx, &node.step, on_err, current_input, expr_ctx)
                        .await
                    {
                        Ok(v) => (v, None),
                        Err(e2) => {
                            if node.allow_failure {
                                let failed_val = failed_allowed_value(&e2);
                                expr_ctx.steps.insert(
                                    node.step.clone(),
                                    StepResult {
                                        output: failed_val.clone(),
                                        confidence: None,
                                    },
                                );
                                return Ok(failed_val);
                            }
                            return Err(e2);
                        }
                    }
                } else if node.allow_failure {
                    let failed_val = failed_allowed_value(&e);
                    expr_ctx.steps.insert(
                        node.step.clone(),
                        StepResult {
                            output: failed_val.clone(),
                            confidence: None,
                        },
                    );
                    return Ok(failed_val);
                } else {
                    return Err(e);
                }
            }
        };

        if let Some(expect) = &node.expect {
            check_expect(&node.step, &handler_out, expect)?;
        }

        expr_ctx.steps.insert(
            node.step.clone(),
            StepResult {
                output: handler_out.clone(),
                confidence,
            },
        );
        Ok(handler_out)
    }

    /// Run a step's `on_error:` fallback handler (#88) as a traced sub-step named
    /// `<step>::on_error`. Static args are expanded against the current `ExprContext`,
    /// same as a normal step's `args`.
    async fn run_on_error(
        &self,
        ctx: &mut CruxCtx,
        step_name: &str,
        on_err: &OnErrorDef,
        current_input: &Value,
        expr_ctx: &ExprContext,
    ) -> Result<Value, CruxErr> {
        let handler = self
            .registry
            .get_handler(&on_err.handler)
            .ok_or_else(|| {
                CruxErr::step_failed(
                    step_name,
                    format!("on_error handler not found: {}", on_err.handler),
                )
            })?
            .clone();

        let input = merge_args(
            current_input.clone(),
            on_err
                .args
                .as_ref()
                .map(|a| expand_args(a.clone(), expr_ctx)),
        );

        let label = format!("{step_name}::on_error");
        run_step_once(ctx, &label, handler, input, None)
            .await
            .map(|(value, _)| value)
    }

    /// Execute a `delegate:` node — looks up a registered agent and runs it via `ctx.step()`.
    // TODO(automation-5): Register CLI agents and preserve child traces while enforcing
    // DelegateNode budgets instead of recording delegation as an ordinary parent step.
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
        let usage_cells: Vec<UsageCell> = node
            .stages
            .iter()
            .map(|_| Arc::new(Mutex::new(None)))
            .collect();

        #[allow(clippy::type_complexity)]
        let stages: Vec<(&str, Box<dyn FnOnce(Value) -> BoxFut<Value> + Send>)> = node
            .stages
            .iter()
            .zip(confidence_cells.iter())
            .zip(usage_cells.iter())
            .map(|((arm, cell), usage_cell)| {
                let handler = registry.get_handler(arm.handler_name()).cloned();
                let name_owned = arm.handler_name().to_string();
                // TODO(automation-9): Apply the same expression expansion semantics to pipe,
                // join, route, and speculate arguments that top-level handler steps receive.
                let static_args = arm.args().cloned();
                let cell = Arc::clone(cell);
                let usage_cell = Arc::clone(usage_cell);
                let stage_fn: Box<dyn FnOnce(Value) -> BoxFut<Value> + Send> =
                    Box::new(move |v: Value| {
                        Box::pin(async move {
                            let h = handler.ok_or_else(|| {
                                CruxErr::step_failed(&name_owned, "handler not found")
                            })?;
                            let input = merge_args(v, static_args);
                            let started = std::time::Instant::now();
                            let execution = h(input).await;
                            *usage_cell.lock().unwrap() =
                                Some((execution.usage, started.elapsed()));
                            let out = execution.outcome?;
                            *cell.lock().unwrap() = out.confidence;
                            Ok(out.value)
                        }) as BoxFut<Value>
                    });
                (arm.label(), stage_fn)
            })
            .collect();

        let input = current_input.clone();
        let result = ctx.pipe(&node.pipe, input, stages).await;
        record_usage_cells(
            ctx,
            node.stages.iter().map(|arm| arm.label()),
            &usage_cells,
            result.as_ref().err(),
        )?;
        let result = result?;

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
        let usage_cells: Vec<UsageCell> = node
            .arms
            .iter()
            .map(|_| Arc::new(Mutex::new(None)))
            .collect();

        let arms: Vec<(&str, BoxFut<Value>)> = node
            .arms
            .iter()
            .zip(confidence_cells.iter())
            .zip(usage_cells.iter())
            .map(|((arm, cell), usage_cell)| {
                let handler = self.registry.get_handler(arm.handler_name()).cloned();
                let input = merge_args(current_input.clone(), arm.args().cloned());
                let name_owned = arm.handler_name().to_string();
                let cell = Arc::clone(cell);
                let usage_cell = Arc::clone(usage_cell);
                let allow_failure = arm.allow_failure();
                let fut: BoxFut<Value> = Box::pin(async move {
                    let h = handler
                        .ok_or_else(|| CruxErr::step_failed(&name_owned, "handler not found"))?;
                    let started = std::time::Instant::now();
                    let execution = h(input).await;
                    *usage_cell.lock().unwrap() = Some((execution.usage, started.elapsed()));
                    match execution.outcome {
                        Ok(out) => {
                            *cell.lock().unwrap() = out.confidence;
                            Ok(out.value)
                        }
                        Err(e) if allow_failure => Ok(failed_allowed_value(&e)),
                        Err(e) => Err(e),
                    }
                });
                (arm.label(), fut)
            })
            .collect();

        let results = ctx.join_all(&node.join_all, arms).await;
        record_usage_cells(
            ctx,
            node.arms.iter().map(|arm| arm.label()),
            &usage_cells,
            results.as_ref().err(),
        )?;
        let results = results?;
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
        let usage_cells: Vec<UsageCell> = node
            .routes
            .iter()
            .map(|_| Arc::new(Mutex::new(None)))
            .collect();

        let routes: Vec<ConfidenceRoute<'_, Value>> = node
            .routes
            .iter()
            .zip(confidence_cells.iter())
            .zip(usage_cells.iter())
            .map(|((branch, cell), usage_cell)| {
                let range = parse_range(&branch.range);
                let handler = self.registry.get_handler(&branch.handler).cloned();
                let input = merge_args(current_input.clone(), branch.args.clone());
                let handler_name = branch.handler.clone();
                let cell = Arc::clone(cell);
                let usage_cell = Arc::clone(usage_cell);
                let fut: BoxFut<Value> = Box::pin(async move {
                    let h = handler
                        .ok_or_else(|| CruxErr::step_failed(&handler_name, "handler not found"))?;
                    let started = std::time::Instant::now();
                    let execution = h(input).await;
                    *usage_cell.lock().unwrap() = Some((execution.usage, started.elapsed()));
                    let out = execution.outcome?;
                    *cell.lock().unwrap() = out.confidence;
                    Ok(out.value)
                });
                (range, branch.label.as_str(), fut)
            })
            .collect();

        let result = ctx
            .route_on_confidence(&node.route_on_confidence, confidence, routes)
            .await;
        record_usage_cells(
            ctx,
            node.routes.iter().map(|branch| branch.label.as_str()),
            &usage_cells,
            result.as_ref().err(),
        )?;
        let result = result?;

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
        let usage_cells: Vec<UsageCell> = node
            .arms
            .iter()
            .map(|_| Arc::new(Mutex::new(None)))
            .collect();
        let arms: Vec<(&str, BoxFut<Value>)> = node
            .arms
            .iter()
            .zip(usage_cells.iter())
            .map(|(arm, usage_cell)| {
                let handler = self.registry.get_handler(arm.handler_name()).cloned();
                let input = merge_args(current_input.clone(), arm.args().cloned());
                let name_owned = arm.handler_name().to_string();
                let usage_cell = Arc::clone(usage_cell);
                let fut: BoxFut<Value> = Box::pin(async move {
                    let h = handler
                        .ok_or_else(|| CruxErr::step_failed(&name_owned, "handler not found"))?;
                    let started = std::time::Instant::now();
                    let execution = h(input).await;
                    *usage_cell.lock().unwrap() = Some((execution.usage, started.elapsed()));
                    execution.outcome.map(|o| o.value)
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
                    .await
            }
            SpeculateMode::FirstOk => builder.first_ok().await,
        };
        record_usage_cells(
            ctx,
            node.arms.iter().map(|arm| arm.label()),
            &usage_cells,
            result.as_ref().err(),
        )?;
        let result = result?;

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

type UsageCell = Arc<Mutex<Option<(HandlerUsage, std::time::Duration)>>>;

// TODO(automation-6): Require metered usage from cost-bearing shell, plugin, and LLM handlers
// and fail closed before work starts when token or USD accounting is unavailable.
fn record_usage_cells<'a>(
    ctx: &mut CruxCtx,
    labels: impl Iterator<Item = &'a str>,
    cells: &[UsageCell],
    source: Option<&CruxErr>,
) -> Result<(), CruxErr> {
    for (label, cell) in labels.zip(cells) {
        if let Some((usage, duration)) = cell.lock().unwrap().take() {
            ctx.record_budget_duration(duration)?;
            if let Err(mut error) = ctx.record_handler_usage(label, usage) {
                if let Some(source) = source {
                    attach_budget_source(&mut error, source.clone());
                }
                return Err(error);
            }
        }
    }
    Ok(())
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

/// Run a single handler invocation through `ctx.step()`, applying an optional
/// per-attempt timeout (#81) and capturing the confidence score via a shared cell
/// (since `ctx.step()` only returns the value). Used directly for non-retrying
/// steps, and once per attempt for steps with a `retry:` policy (#79).
async fn run_step_once(
    ctx: &mut CruxCtx,
    step_label: &str,
    handler: crate::registry::BoxHandler,
    input: Value,
    timeout_ms: Option<u64>,
) -> Result<(Value, Option<f32>), CruxErr> {
    let started = std::time::Instant::now();
    let confidence_cell: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));
    let usage_cell: Arc<Mutex<Option<HandlerUsage>>> = Arc::new(Mutex::new(None));
    let cc = confidence_cell.clone();
    let uc = usage_cell.clone();
    let step_name_owned = step_label.to_string();
    let result = ctx
        .step_budgeted(step_label, move || async move {
            let fut = async move {
                let execution = handler(input).await;
                *uc.lock().unwrap() = Some(execution.usage);
                let raw = execution.outcome?;
                *cc.lock().unwrap() = raw.confidence;
                Ok::<Value, CruxErr>(raw.value)
            };
            match timeout_ms {
                Some(ms) => {
                    match tokio::time::timeout(std::time::Duration::from_millis(ms), fut).await {
                        Ok(res) => res,
                        Err(_) => Err(CruxErr::step_failed(
                            &step_name_owned,
                            format!("step timed out after {ms}ms"),
                        )),
                    }
                }
                None => fut.await,
            }
        })
        .await;
    ctx.record_budget_duration(started.elapsed())?;
    let usage = usage_cell
        .lock()
        .unwrap()
        .unwrap_or_else(HandlerUsage::unreported);
    if let Err(mut accounting_error) = ctx.record_handler_usage(step_label, usage) {
        if let Err(source) = result {
            attach_budget_source(&mut accounting_error, source);
        }
        return Err(accounting_error);
    }
    result.map(|v| (v, *confidence_cell.lock().unwrap()))
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

/// Build the placeholder output value for a step/arm whose failure was tolerated
/// via `allow_failure: true` (#80). Carries enough metadata for downstream
/// expressions/consumers to detect and inspect the failure without a panic.
fn failed_allowed_value(err: &CruxErr) -> Value {
    serde_json::json!({
        "status": "failed_allowed",
        "error": err.to_string(),
    })
}

/// Evaluate a step's `expect:` clause against its handler output.
///
/// Looks up `exit_code`, `stdout`, `stderr` fields on the output value (the
/// convention used by shell-style handlers). Any configured check that fails
/// produces a descriptive `CruxErr::StepFailed`. Checks not configured in the
/// `expect:` block are skipped.
fn check_expect(step_name: &str, output: &Value, expect: &ExpectDef) -> Result<(), CruxErr> {
    if let Some(expected_code) = expect.exit_code {
        let actual = output.get("exit_code").and_then(|v| v.as_i64());
        if actual != Some(expected_code) {
            return Err(CruxErr::step_failed(
                step_name,
                format!(
                    "expect.exit_code mismatch: expected {expected_code}, got {}",
                    actual
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "<missing>".to_string())
                ),
            ));
        }
    }

    if let Some(needle) = &expect.stdout_contains {
        let actual = output.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
        if !actual.contains(needle.as_str()) {
            return Err(CruxErr::step_failed(
                step_name,
                format!(
                    "expect.stdout_contains mismatch: stdout did not contain {needle:?} (stdout was {actual:?})"
                ),
            ));
        }
    }

    if let Some(needle) = &expect.stderr_contains {
        let actual = output.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
        if !actual.contains(needle.as_str()) {
            return Err(CruxErr::step_failed(
                step_name,
                format!(
                    "expect.stderr_contains mismatch: stderr did not contain {needle:?} (stderr was {actual:?})"
                ),
            ));
        }
    }

    Ok(())
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
    if let Some(steps) = def.steps.or(def.calls) {
        budgets.push(Budget::steps(steps));
    }
    if let Some(duration_ms) = def.duration_ms {
        budgets.push(Budget::duration(std::time::Duration::from_millis(
            duration_ms,
        )));
    }
    if let Some(usd) = def.usd {
        budgets.push(Budget::usd(usd));
    } else if let Some(cost_cents) = def.cost_cents {
        budgets.push(Budget::usd(UsdAmount::from_micros(
            cost_cents.saturating_mul(10_000),
        )));
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
