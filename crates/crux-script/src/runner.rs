//! Validation, execution, replay, and control-flow interpretation for pipelines.

/// Pipeline runner — interprets a parsed YAML pipeline against CruxCtx + HandlerRegistry.
use std::sync::{Arc, Mutex};

use crux_runtime::prelude::*;
use crux_types::budget::{Budget, HandlerUsage, UsdAmount};
use crux_types::crux_value::Crux;
use crux_types::error::CruxErr;
use indexmap::IndexMap;
use serde_json::Value;

use crate::expr::IterFrame;
use crate::expr::{ExprContext, ExprError, StepResult};
use crate::ir::{
    RuntimeScopes, TypedHandlerStep, TypedPipeline, TypedRecoveryStep, TypedStep, TypedStepKind,
};
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
    /// Creates a runner backed by a shared handler and agent registry.
    pub fn new(registry: Arc<HandlerRegistry>) -> Self {
        Self { registry }
    }

    /// Run a pipeline definition with the given input, producing a full Crux trace.
    ///
    /// Validates the pipeline against the handler registry before execution.
    /// If validation produces any errors, returns immediately with a failed trace.
    /// Warnings are printed to stderr.
    pub async fn run(&self, pipeline: &PipelineDef, input: Value) -> Crux<Value> {
        let compilation = crate::compile_pipeline(
            pipeline,
            &self.registry,
            crate::CompileOptions::permissive(),
        );
        for diagnostic in compilation.diagnostics() {
            if diagnostic.severity == crate::validator::DiagnosticSeverity::Warning
                && diagnostic.code != crate::ValidationCode::MissingContract
            {
                eprintln!(
                    "[crux] warning: {}: {}",
                    diagnostic.location, diagnostic.message
                );
            }
        }
        let diagnostics = compilation.diagnostics().to_vec();
        let compilation_ok = compilation.is_ok();
        if let Some(compiled) = compilation.into_artifact() {
            if compiled.supports_simple_execution() && compiled.step_count() == pipeline.steps.len()
            {
                return self.run_compiled(&compiled, input).await;
            }
            return self
                .run_core(pipeline, input, None, ReplayMode::Strict)
                .await;
        }
        let has_unknown_executor = diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code,
                crate::ValidationCode::UnknownHandler | crate::ValidationCode::UnknownAgent
            )
        });
        let only_legacy_reference_errors = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == crate::validator::DiagnosticSeverity::Error)
            .all(|diagnostic| diagnostic.code == crate::ValidationCode::UnknownReference);
        if (compilation_ok || only_legacy_reference_errors) && !has_unknown_executor {
            return self
                .run_core(pipeline, input, None, ReplayMode::Strict)
                .await;
        }
        compilation_failure(&pipeline.pipeline, &diagnostics)
    }

    /// Execute a pipeline whose handlers were resolved during compilation.
    pub async fn run_compiled(&self, pipeline: &TypedPipeline, input: Value) -> Crux<Value> {
        self.run_compiled_core(pipeline, input, None, ReplayMode::Strict)
            .await
    }

    /// Execute a compiled pipeline using values cached in a previous trace.
    ///
    /// Typed pipelines that read handler-reported confidence cannot be replayed
    /// because replay traces currently retain handler values but not confidence.
    pub async fn run_compiled_with_replay(
        &self,
        pipeline: &TypedPipeline,
        input: Value,
        previous: &Crux<Value>,
        mode: ReplayMode,
    ) -> Crux<Value> {
        if pipeline.is_confidence_dependent() {
            let ctx = CruxCtx::new(&pipeline.name);
            return ctx.finalize(Err(CruxErr::step_failed(
                &pipeline.name,
                "confidence-dependent typed pipelines cannot be replayed",
            )));
        }
        self.run_compiled_core(pipeline, input, Some(previous), mode)
            .await
    }

    async fn run_compiled_core(
        &self,
        pipeline: &TypedPipeline,
        input: Value,
        previous: Option<&Crux<Value>>,
        mode: ReplayMode,
    ) -> Crux<Value> {
        let mut ctx = CruxCtx::new(&pipeline.name);
        if let Some(schema) = &pipeline.input_schema
            && let Err(violation) = schema.validate(&input)
        {
            return ctx.finalize(Err(CruxErr::step_failed(
                &pipeline.name,
                format!("pipeline input contract violated: {violation}"),
            )));
        }
        if let Some(budget_def) = &pipeline.budget {
            match budget_from_def(budget_def) {
                Ok(budget) => ctx.set_budget(budget),
                Err(error) => return ctx.finalize(Err(error)),
            }
        }
        if let Some(previous) = previous {
            ctx.set_replay_mode(mode);
            ctx.replay_from(previous);
        }

        let mut expr_ctx = ExprContext::new(input.clone());
        let mut scopes = RuntimeScopes::default();
        let mut variables = pipeline.variables.iter().collect::<Vec<_>>();
        variables.sort_by_key(|(_, binding)| binding.id.0);
        for (name, binding) in variables {
            match binding.value.evaluate(&expr_ctx) {
                Ok(value) => {
                    expr_ctx.vars.insert(name.clone(), value);
                }
                Err(error) => {
                    return ctx.finalize(Err(CruxErr::step_failed(name, error.to_string())));
                }
            }
        }

        let result = self
            .execute_typed_steps(&mut ctx, &pipeline.steps, input, &mut expr_ctx, &mut scopes)
            .await;
        ctx.finalize(result)
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
        if let Some(crux) = self.compile_or_fail(pipeline) {
            return crux;
        }
        self.run_core(pipeline, input, Some(previous), mode).await
    }

    /// Run without pre-flight validation. Use for tests or REPL.
    pub async fn run_unchecked(&self, pipeline: &PipelineDef, input: Value) -> Crux<Value> {
        self.run_core(pipeline, input, None, ReplayMode::Strict)
            .await
    }

    /// Compile through the compatibility validation view, preserving raw replay execution.
    fn compile_or_fail(&self, pipeline: &PipelineDef) -> Option<Crux<Value>> {
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
            match budget_from_def(budget_def) {
                Ok(budget) => ctx.set_budget(budget),
                Err(error) => return ctx.finalize(Err(error)),
            }
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
            match budget_from_def(budget_def) {
                Ok(budget) => ctx.set_budget(budget),
                Err(error) => return ctx.finalize(Err(error)),
            }
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
    // TODO(feature-idea-17): Add bounded parallel iteration with deterministic trace merging.
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

    async fn execute_typed_handler_step(
        &self,
        ctx: &mut CruxCtx,
        handler: &TypedHandlerStep,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
        scopes: &RuntimeScopes,
    ) -> Result<Value, CruxErr> {
        let args = handler
            .args
            .as_ref()
            .map(|args| args.evaluate_scoped(expr_ctx, scopes))
            .transpose()
            .map_err(|error| CruxErr::step_failed(&handler.node.step, error.to_string()))?
            .unwrap_or(Value::Null);
        let invocation = crate::StepInvocation::new(current_input.clone(), args);
        let (max_attempts, delay_ms) = match &handler.node.retry {
            Some(retry) => (retry.count + 1, retry.delay_ms),
            None => (1, 0),
        };

        let mut last_error = None;
        let mut success = None;
        for attempt in 0..max_attempts {
            let label = if handler.node.retry.is_some() {
                format!("{}::attempt{}", handler.node.step, attempt + 1)
            } else {
                handler.node.step.clone()
            };
            match run_typed_step_once(
                ctx,
                &label,
                Arc::clone(&handler.runner),
                invocation.clone(),
                handler.node.timeout_ms,
            )
            .await
            {
                Ok(output) => {
                    success = Some(output);
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    if attempt + 1 < max_attempts && delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        let (output, confidence) = match success {
            Some(output) => output,
            None => {
                let error = last_error.expect("a failed attempt records an error");
                if let Some(recovery) = &handler.recovery {
                    match self
                        .execute_typed_recovery(
                            ctx,
                            &handler.node.step,
                            recovery,
                            current_input,
                            expr_ctx,
                            scopes,
                        )
                        .await
                    {
                        Ok(value) => (value, None),
                        Err(recovery_error) if handler.node.allow_failure => {
                            (failed_allowed_value(&recovery_error), None)
                        }
                        Err(recovery_error) => return Err(recovery_error),
                    }
                } else if handler.node.allow_failure {
                    (failed_allowed_value(&error), None)
                } else {
                    return Err(error);
                }
            }
        };

        if let Some(expect) = &handler.node.expect {
            check_expect(&handler.node.step, &output, expect)?;
        }
        expr_ctx.steps.insert(
            handler.node.step.clone(),
            StepResult {
                output: output.clone(),
                confidence,
            },
        );
        Ok(output)
    }

    async fn execute_typed_recovery(
        &self,
        ctx: &mut CruxCtx,
        step_name: &str,
        recovery: &TypedRecoveryStep,
        current_input: &Value,
        expr_ctx: &ExprContext,
        scopes: &RuntimeScopes,
    ) -> Result<Value, CruxErr> {
        let args = recovery
            .args
            .as_ref()
            .map(|args| args.evaluate_scoped(expr_ctx, scopes))
            .transpose()
            .map_err(|error| CruxErr::step_failed(step_name, error.to_string()))?
            .unwrap_or(Value::Null);
        let label = format!("{step_name}::on_error");
        run_typed_step_once(
            ctx,
            &label,
            Arc::clone(&recovery.runner),
            crate::StepInvocation::new(current_input.clone(), args),
            None,
        )
        .await
        .map(|(value, _)| value)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_typed_pipe(
        &self,
        ctx: &mut CruxCtx,
        step: &TypedStep,
        node: &PipeNode,
        stages: &[crate::ir::TypedArm],
        current: Value,
        expr_ctx: &mut ExprContext,
        scopes: &RuntimeScopes,
    ) -> Result<Value, CruxErr> {
        let confidence_cells = typed_confidence_cells(stages.len());
        let usage_cells = typed_usage_cells(stages.len());
        let mut runtime_stages: Vec<RecoverablePipeStage<'_, Value>> =
            Vec::with_capacity(stages.len());
        for ((stage, confidence), usage) in stages
            .iter()
            .zip(confidence_cells.iter())
            .zip(usage_cells.iter())
        {
            let args = evaluate_typed_args(stage.args.as_ref(), expr_ctx, scopes, &step.name)?;
            let runner = Arc::clone(&stage.runner);
            let label = stage.node.label();
            let step_label = format!("{}::{label}", node.pipe);
            let confidence = Arc::clone(confidence);
            let usage = Arc::clone(usage);
            let run = Box::new(move |input| {
                Box::pin(run_typed_runner_direct(
                    step_label,
                    runner,
                    crate::StepInvocation::new(input, args),
                    confidence,
                    usage,
                )) as BoxFut<Value>
            });
            let policy = if stage.node.allow_failure() {
                PipeFailurePolicy::SubstituteWith(Box::new(|error| {
                    Ok(failed_allowed_value(&error))
                }))
            } else {
                PipeFailurePolicy::Propagate
            };
            runtime_stages.push((label, run, policy));
        }
        let output = ctx
            .pipe_with_recovery(&node.pipe, current, runtime_stages)
            .await;
        record_usage_cells(
            ctx,
            stages.iter().map(|stage| stage.node.label()),
            &usage_cells,
            output.as_ref().err(),
        )?;
        let output = output?;
        let confidence = confidence_cells.last().and_then(typed_confidence);
        record_typed_result(expr_ctx, &step.name, &output, confidence);
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_typed_join(
        &self,
        ctx: &mut CruxCtx,
        step: &TypedStep,
        node: &JoinAllNode,
        arms: &[crate::ir::TypedArm],
        current: &Value,
        expr_ctx: &mut ExprContext,
        scopes: &RuntimeScopes,
    ) -> Result<Value, CruxErr> {
        let confidence_cells = typed_confidence_cells(arms.len());
        let usage_cells = typed_usage_cells(arms.len());
        let mut runtime_arms = Vec::with_capacity(arms.len());
        for ((arm, confidence), usage) in arms
            .iter()
            .zip(confidence_cells.iter())
            .zip(usage_cells.iter())
        {
            let args = evaluate_typed_args(arm.args.as_ref(), expr_ctx, scopes, &step.name)?;
            let runner = Arc::clone(&arm.runner);
            let label = arm.node.label();
            let step_label = format!("{}::{label}", node.join_all);
            let confidence = Arc::clone(confidence);
            let usage = Arc::clone(usage);
            let input = current.clone();
            let allow_failure = arm.node.allow_failure();
            let future: BoxFut<Value> = Box::pin(async move {
                match run_typed_runner_direct(
                    step_label,
                    runner,
                    crate::StepInvocation::new(input, args),
                    confidence,
                    usage,
                )
                .await
                {
                    Err(error) if allow_failure => Ok(failed_allowed_value(&error)),
                    result => result,
                }
            });
            runtime_arms.push((label, future));
        }
        let results = ctx.join_all(&node.join_all, runtime_arms).await;
        record_usage_cells(
            ctx,
            arms.iter().map(|arm| arm.node.label()),
            &usage_cells,
            results.as_ref().err(),
        )?;
        let output = Value::Array(results?);
        let scores = confidence_cells
            .iter()
            .filter_map(typed_confidence)
            .collect::<Vec<_>>();
        let confidence =
            (!scores.is_empty()).then(|| scores.iter().sum::<f32>() / scores.len() as f32);
        record_typed_result(expr_ctx, &step.name, &output, confidence);
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_typed_route(
        &self,
        ctx: &mut CruxCtx,
        step: &TypedStep,
        node: &RouteNode,
        value: &crate::ir::TypedValue,
        branches: &[crate::ir::TypedRouteBranch],
        current: &Value,
        expr_ctx: &mut ExprContext,
        scopes: &RuntimeScopes,
    ) -> Result<Value, CruxErr> {
        let routing_confidence = value
            .evaluate_scoped(expr_ctx, scopes)
            .map_err(|error| CruxErr::step_failed(&step.name, error.to_string()))?
            .as_f64()
            .map(|value| value as f32)
            .ok_or_else(|| CruxErr::step_failed(&step.name, ExprError::NotNumeric.to_string()))?;
        let confidence_cells = typed_confidence_cells(branches.len());
        let usage_cells = typed_usage_cells(branches.len());
        let mut routes = Vec::with_capacity(branches.len());
        for ((branch, confidence), usage) in branches
            .iter()
            .zip(confidence_cells.iter())
            .zip(usage_cells.iter())
        {
            let args = evaluate_typed_args(branch.args.as_ref(), expr_ctx, scopes, &step.name)?;
            let runner = Arc::clone(&branch.runner);
            let step_label = format!("{}::{}", node.route_on_confidence, branch.node.label);
            let confidence = Arc::clone(confidence);
            let usage = Arc::clone(usage);
            let input = current.clone();
            routes.push((
                parse_range(&branch.node.range),
                branch.node.label.as_str(),
                Box::pin(run_typed_runner_direct(
                    step_label,
                    runner,
                    crate::StepInvocation::new(input, args),
                    confidence,
                    usage,
                )) as BoxFut<Value>,
            ));
        }
        let output = ctx
            .route_on_confidence(&node.route_on_confidence, routing_confidence, routes)
            .await;
        record_usage_cells(
            ctx,
            branches.iter().map(|branch| branch.node.label.as_str()),
            &usage_cells,
            output.as_ref().err(),
        )?;
        let output = output?;
        let confidence = confidence_cells
            .iter()
            .find_map(typed_confidence)
            .or(Some(routing_confidence));
        record_typed_result(expr_ctx, &step.name, &output, confidence);
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_typed_speculation(
        &self,
        ctx: &mut CruxCtx,
        step: &TypedStep,
        node: &SpeculateNode,
        arms: &[crate::ir::TypedArm],
        current: &Value,
        expr_ctx: &mut ExprContext,
        scopes: &RuntimeScopes,
    ) -> Result<Value, CruxErr> {
        let confidence_cells = typed_confidence_cells(arms.len());
        let usage_cells = typed_usage_cells(arms.len());
        let mut runtime_arms = Vec::with_capacity(arms.len());
        for ((arm, confidence), usage) in arms
            .iter()
            .zip(confidence_cells.iter())
            .zip(usage_cells.iter())
        {
            let args = evaluate_typed_args(arm.args.as_ref(), expr_ctx, scopes, &step.name)?;
            let runner = Arc::clone(&arm.runner);
            let step_label = format!("{}::{}", node.speculate, arm.node.label());
            let confidence = Arc::clone(confidence);
            let usage = Arc::clone(usage);
            let input = current.clone();
            runtime_arms.push((
                arm.node.label(),
                Box::pin(run_typed_runner_direct(
                    step_label,
                    runner,
                    crate::StepInvocation::new(input, args),
                    confidence,
                    usage,
                )) as BoxFut<Value>,
            ));
        }
        let builder = ctx.speculate(&node.speculate, runtime_arms);
        let mut usage_iter = usage_cells.iter();
        let report = move |_arm: &str| {
            usage_iter
                .next()
                .and_then(|cell| cell.lock().unwrap().take())
        };
        let output = match node.mode {
            SpeculateMode::PickBest => {
                builder
                    .pick_best_by_metered(
                        |value: &Value| {
                            value.get("score").and_then(Value::as_f64).unwrap_or(0.0) as f32
                        },
                        report,
                    )
                    .await
            }
            SpeculateMode::FirstOk => builder.first_ok_metered(report).await,
        }?;
        record_typed_result(expr_ctx, &step.name, &output, None);
        Ok(output)
    }

    async fn execute_typed_loop(
        &self,
        ctx: &mut CruxCtx,
        step: &TypedStep,
        current: Value,
        expr_ctx: &mut ExprContext,
        scopes: &mut RuntimeScopes,
    ) -> Result<Value, CruxErr> {
        match &step.kind {
            TypedStepKind::ForEach {
                items,
                bindings,
                body,
                break_if,
                ..
            } => {
                let values = items
                    .evaluate_scoped(expr_ctx, scopes)
                    .map_err(|error| CruxErr::step_failed(&step.name, error.to_string()))?
                    .as_array()
                    .cloned()
                    .ok_or_else(|| {
                        CruxErr::step_failed(
                            &step.name,
                            "for_each items did not resolve to an array",
                        )
                    })?;
                self.execute_typed_iterations(
                    ctx,
                    step,
                    bindings,
                    body,
                    break_if.as_ref(),
                    current,
                    values.into_iter().map(Some),
                    expr_ctx,
                    scopes,
                )
                .await
            }
            TypedStepKind::While {
                bindings,
                condition,
                body,
                break_if,
                ..
            } => {
                let mut output = current;
                let mut index = 0;
                while evaluate_typed_bool(condition, expr_ctx, scopes, &step.name)? {
                    let (value, should_break) = self
                        .execute_typed_iteration(
                            ctx,
                            step,
                            bindings,
                            body,
                            break_if.as_ref(),
                            output,
                            index,
                            None,
                            expr_ctx,
                            scopes,
                        )
                        .await?;
                    output = value;
                    index += 1;
                    if should_break {
                        break;
                    }
                }
                record_typed_result(expr_ctx, &step.name, &output, None);
                Ok(output)
            }
            TypedStepKind::Repeat {
                node,
                bindings,
                body,
                break_if,
            } => {
                self.execute_typed_iterations(
                    ctx,
                    step,
                    bindings,
                    body,
                    break_if.as_ref(),
                    current,
                    (0..node.count).map(|_| None),
                    expr_ctx,
                    scopes,
                )
                .await
            }
            TypedStepKind::Poll {
                node,
                bindings,
                body,
                until,
            } => {
                let mut output = current;
                let mut index = 0_u32;
                loop {
                    let (value, done) = self
                        .execute_typed_iteration(
                            ctx,
                            step,
                            bindings,
                            body,
                            Some(until),
                            output,
                            index as usize,
                            None,
                            expr_ctx,
                            scopes,
                        )
                        .await?;
                    output = value;
                    index += 1;
                    if done || node.max_attempts.is_some_and(|max| index >= max) {
                        break;
                    }
                    if let Some(milliseconds) = node.interval_ms {
                        tokio::time::sleep(std::time::Duration::from_millis(milliseconds)).await;
                    }
                }
                record_typed_result(expr_ctx, &step.name, &output, None);
                Ok(output)
            }
            _ => unreachable!("execute_typed_loop only accepts typed loop nodes"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_typed_iterations<I>(
        &self,
        ctx: &mut CruxCtx,
        step: &TypedStep,
        bindings: &crate::ir::TypedLoopBindings,
        body: &[TypedStep],
        break_if: Option<&crate::ir::TypedValue>,
        mut output: Value,
        items: I,
        expr_ctx: &mut ExprContext,
        scopes: &mut RuntimeScopes,
    ) -> Result<Value, CruxErr>
    where
        I: IntoIterator<Item = Option<Value>>,
    {
        for (index, item) in items.into_iter().enumerate() {
            let (value, should_break) = self
                .execute_typed_iteration(
                    ctx, step, bindings, body, break_if, output, index, item, expr_ctx, scopes,
                )
                .await?;
            output = value;
            if should_break {
                break;
            }
        }
        record_typed_result(expr_ctx, &step.name, &output, None);
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_typed_iteration(
        &self,
        ctx: &mut CruxCtx,
        step: &TypedStep,
        bindings: &crate::ir::TypedLoopBindings,
        body: &[TypedStep],
        break_if: Option<&crate::ir::TypedValue>,
        input: Value,
        index: usize,
        item: Option<Value>,
        expr_ctx: &mut ExprContext,
        scopes: &mut RuntimeScopes,
    ) -> Result<(Value, bool), CruxErr> {
        scopes.push_iteration(bindings, index, item);
        let iteration = self
            .execute_typed_steps(ctx, body, input, expr_ctx, scopes)
            .await;
        let result = match iteration {
            Ok(output) => {
                let marker = output.clone();
                match ctx
                    .step(&format!("{}[{index}]", step.name), move || async move {
                        Ok::<Value, CruxErr>(marker)
                    })
                    .await
                {
                    Ok(_) => break_if
                        .map(|expression| {
                            evaluate_typed_bool(expression, expr_ctx, scopes, &step.name)
                        })
                        .transpose()
                        .map(|should_break| (output, should_break.unwrap_or(false))),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        scopes.pop_iteration();
        result
    }

    fn execute_typed_steps<'a>(
        &'a self,
        ctx: &'a mut CruxCtx,
        steps: &'a [TypedStep],
        input: Value,
        expr_ctx: &'a mut ExprContext,
        scopes: &'a mut RuntimeScopes,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, CruxErr>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut current = input;
            for step in steps {
                current = match &step.kind {
                    TypedStepKind::Handler(handler) => {
                        self.execute_typed_handler_step(ctx, handler, &current, expr_ctx, scopes)
                            .await?
                    }
                    TypedStepKind::Pipe { node, stages } => {
                        self.execute_typed_pipe(ctx, step, node, stages, current, expr_ctx, scopes)
                            .await?
                    }
                    TypedStepKind::JoinAll { node, arms } => {
                        self.execute_typed_join(ctx, step, node, arms, &current, expr_ctx, scopes)
                            .await?
                    }
                    TypedStepKind::RouteOnConfidence {
                        node,
                        value,
                        branches,
                    } => {
                        self.execute_typed_route(
                            ctx, step, node, value, branches, &current, expr_ctx, scopes,
                        )
                        .await?
                    }
                    TypedStepKind::Speculate { node, arms } => {
                        self.execute_typed_speculation(
                            ctx, step, node, arms, &current, expr_ctx, scopes,
                        )
                        .await?
                    }
                    TypedStepKind::Poll { .. }
                    | TypedStepKind::ForEach { .. }
                    | TypedStepKind::While { .. }
                    | TypedStepKind::Repeat { .. } => {
                        self.execute_typed_loop(ctx, step, current, expr_ctx, scopes)
                            .await?
                    }
                };
            }
            Ok(current)
        })
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
    // TODO(feature-idea-13): Use runtime delegation so YAML preserves child traces and budgets.
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

    /// Execute a `pipe:` node, accounting each sequential stage before the next.
    async fn execute_pipe_step(
        &self,
        ctx: &mut CruxCtx,
        node: &PipeNode,
        current_input: &Value,
        expr_ctx: &mut ExprContext,
    ) -> Result<Value, CruxErr> {
        let mut output = current_input.clone();
        let mut confidence = None;

        for stage in &node.stages {
            let handler = self
                .registry
                .get_handler(stage.handler_name())
                .ok_or_else(|| CruxErr::step_failed(stage.handler_name(), "handler not found"))?
                .clone();
            let input = merge_args(output, stage.args().cloned());
            let step_name = format!("{}::{}", node.pipe, stage.label());
            match run_step_once(ctx, &step_name, handler, input, None).await {
                Ok((value, stage_confidence)) => {
                    output = value;
                    confidence = stage_confidence;
                }
                Err(error @ CruxErr::StepFailed { .. }) if stage.allow_failure() => {
                    output = failed_allowed_value(&error);
                    confidence = None;
                }
                Err(error) => return Err(error),
            }
        }

        expr_ctx.steps.insert(
            node.pipe.clone(),
            StepResult {
                output: output.clone(),
                confidence,
            },
        );
        Ok(output)
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
        let mut usage_iter = usage_cells.iter();
        let report = move |_arm: &str| {
            usage_iter
                .next()
                .and_then(|cell| cell.lock().unwrap().take())
        };
        let result = match node.mode {
            SpeculateMode::PickBest => {
                builder
                    .pick_best_by_metered(
                        |v: &Value| v.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0) as f32,
                        report,
                    )
                    .await
            }
            SpeculateMode::FirstOk => builder.first_ok_metered(report).await,
        }?;

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
type ConfidenceCell = Arc<Mutex<Option<f32>>>;

fn typed_usage_cells(count: usize) -> Vec<UsageCell> {
    (0..count).map(|_| Arc::new(Mutex::new(None))).collect()
}

fn typed_confidence_cells(count: usize) -> Vec<ConfidenceCell> {
    (0..count).map(|_| Arc::new(Mutex::new(None))).collect()
}

fn typed_confidence(cell: &ConfidenceCell) -> Option<f32> {
    *cell.lock().unwrap()
}

fn evaluate_typed_args(
    args: Option<&crate::ir::TypedValue>,
    expr_ctx: &ExprContext,
    scopes: &RuntimeScopes,
    step: &str,
) -> Result<Value, CruxErr> {
    args.map(|args| args.evaluate_scoped(expr_ctx, scopes))
        .transpose()
        .map_err(|error| CruxErr::step_failed(step, error.to_string()))
        .map(|args| args.unwrap_or(Value::Null))
}

fn evaluate_typed_bool(
    expression: &crate::ir::TypedValue,
    expr_ctx: &ExprContext,
    scopes: &RuntimeScopes,
    step: &str,
) -> Result<bool, CruxErr> {
    expression
        .evaluate_scoped(expr_ctx, scopes)
        .map_err(|error| CruxErr::step_failed(step, error.to_string()))?
        .as_bool()
        .ok_or_else(|| CruxErr::step_failed(step, ExprError::NotBoolean.to_string()))
}

fn record_typed_result(
    expr_ctx: &mut ExprContext,
    name: &str,
    output: &Value,
    confidence: Option<f32>,
) {
    expr_ctx.steps.insert(
        name.to_string(),
        StepResult {
            output: output.clone(),
            confidence,
        },
    );
}

async fn run_typed_runner_direct(
    step_label: String,
    runner: Arc<dyn crate::StepRunner>,
    invocation: crate::StepInvocation,
    confidence_cell: ConfidenceCell,
    usage_cell: UsageCell,
) -> Result<Value, CruxErr> {
    validate_handler_invocation(&step_label, runner.metadata(), &invocation)?;
    let started = std::time::Instant::now();
    let execution = runner.run(invocation).await;
    *usage_cell.lock().unwrap() = Some((execution.usage, started.elapsed()));
    let output = execution.outcome?;
    validate_handler_output(&step_label, runner.metadata(), &output)?;
    *confidence_cell.lock().unwrap() = output.confidence;
    Ok(output.value)
}

fn compilation_failure(name: &str, diagnostics: &[crate::ValidationDiagnostic]) -> Crux<Value> {
    let details = diagnostics
        .iter()
        .map(|diagnostic| format!("{}: {}", diagnostic.location, diagnostic.message))
        .collect::<Vec<_>>()
        .join("\n");
    let ctx = CruxCtx::new(name);
    ctx.finalize(Err(CruxErr::step_failed(
        name,
        format!("pipeline validation/compilation failed:\n{details}"),
    )))
}

// TODO(automation-6): Require metered usage from cost-bearing shell, plugin, and LLM handlers
// and fail closed before work starts when token or USD accounting is unavailable.
fn record_usage_cells<'a, M: InvocationMeter>(
    ctx: &mut M,
    labels: impl Iterator<Item = &'a str>,
    cells: &[UsageCell],
    source: Option<&CruxErr>,
) -> Result<(), CruxErr> {
    let mut first_label = None;
    let mut total_tokens = 0_u64;
    let mut total_duration = std::time::Duration::ZERO;
    let mut total_usd = Some(UsdAmount::ZERO);
    for (label, cell) in labels.zip(cells) {
        if let Some((usage, duration)) = cell.lock().unwrap().take() {
            first_label.get_or_insert(label);
            total_tokens = total_tokens.saturating_add(usage.tokens);
            total_duration = total_duration.saturating_add(duration);
            total_usd = match (total_usd, usage.usd) {
                (Some(total), Some(amount)) => total.checked_add(amount),
                _ => None,
            };
        }
    }
    let Some(label) = first_label else {
        return Ok(());
    };
    let usage = HandlerUsage {
        tokens: total_tokens,
        usd: total_usd,
    };
    ctx.record_invocation_usage(label, usage, total_duration)
        .map_err(|mut error| {
            if let Some(source) = source {
                attach_budget_source(&mut error, source.clone());
            }
            error
        })
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
async fn run_step_once<M: InvocationMeter>(
    ctx: &mut M,
    step_label: &str,
    handler: crate::registry::BoxHandler,
    input: Value,
    timeout_ms: Option<u64>,
) -> Result<(Value, Option<f32>), CruxErr> {
    let started_cell: Arc<Mutex<Option<std::time::Instant>>> = Arc::new(Mutex::new(None));
    let confidence_cell: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));
    let usage_cell: Arc<Mutex<Option<HandlerUsage>>> = Arc::new(Mutex::new(None));
    let sc = started_cell.clone();
    let cc = confidence_cell.clone();
    let uc = usage_cell.clone();
    let step_name_owned = step_label.to_string();
    let invocation = ctx
        .invoke_budgeted_step(step_label, move || async move {
            *sc.lock().unwrap() = Some(std::time::Instant::now());
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
    if !invocation.executed {
        return invocation
            .outcome
            .map(|value| (value, *confidence_cell.lock().unwrap()));
    }

    let usage = usage_cell
        .lock()
        .unwrap()
        .unwrap_or_else(HandlerUsage::unreported);
    let duration = started_cell
        .lock()
        .unwrap()
        .expect("executed invocation records its live start")
        .elapsed();
    if let Err(mut accounting_error) = ctx.record_invocation_usage(step_label, usage, duration) {
        if let Err(source) = invocation.outcome {
            attach_budget_source(&mut accounting_error, source);
        }
        return Err(accounting_error);
    }
    invocation
        .outcome
        .map(|v| (v, *confidence_cell.lock().unwrap()))
}

async fn run_typed_step_once<M: InvocationMeter>(
    ctx: &mut M,
    step_label: &str,
    runner: Arc<dyn crate::StepRunner>,
    invocation: crate::StepInvocation,
    timeout_ms: Option<u64>,
) -> Result<(Value, Option<f32>), CruxErr> {
    validate_handler_invocation(step_label, runner.metadata(), &invocation)?;
    let started_cell = Arc::new(Mutex::new(None));
    let confidence_cell = Arc::new(Mutex::new(None));
    let usage_cell = Arc::new(Mutex::new(None));
    let started = Arc::clone(&started_cell);
    let confidence = Arc::clone(&confidence_cell);
    let usage = Arc::clone(&usage_cell);
    let step_name = step_label.to_string();
    let invocation_result = ctx
        .invoke_budgeted_step(step_label, move || async move {
            *started.lock().unwrap() = Some(std::time::Instant::now());
            let output_step_name = step_name.clone();
            let future = async move {
                let execution = runner.run(invocation).await;
                *usage.lock().unwrap() = Some(execution.usage);
                let output = execution.outcome?;
                validate_handler_output(&output_step_name, runner.metadata(), &output)?;
                *confidence.lock().unwrap() = output.confidence;
                Ok::<Value, CruxErr>(output.value)
            };
            match timeout_ms {
                Some(milliseconds) => match tokio::time::timeout(
                    std::time::Duration::from_millis(milliseconds),
                    future,
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(CruxErr::step_failed(
                        &step_name,
                        format!("step timed out after {milliseconds}ms"),
                    )),
                },
                None => future.await,
            }
        })
        .await;
    if !invocation_result.executed {
        return invocation_result
            .outcome
            .map(|value| (value, *confidence_cell.lock().unwrap()));
    }

    let usage = usage_cell
        .lock()
        .unwrap()
        .unwrap_or_else(HandlerUsage::unreported);
    let duration = started_cell
        .lock()
        .unwrap()
        .expect("executed invocation records its live start")
        .elapsed();
    if let Err(mut accounting_error) = ctx.record_invocation_usage(step_label, usage, duration) {
        if let Err(source) = invocation_result.outcome {
            attach_budget_source(&mut accounting_error, source);
        }
        return Err(accounting_error);
    }
    invocation_result
        .outcome
        .map(|value| (value, *confidence_cell.lock().unwrap()))
}

fn validate_handler_invocation(
    step: &str,
    metadata: &crate::HandlerMetadata,
    invocation: &crate::StepInvocation,
) -> Result<(), CruxErr> {
    if let Some(schema) = &metadata.input_schema {
        schema.validate(invocation.input()).map_err(|violation| {
            CruxErr::step_failed(
                step,
                format!(
                    "handler '{}' input contract violated: {violation}",
                    metadata.name
                ),
            )
        })?;
    }
    validate_handler_args(step, metadata, invocation.args())
}

fn validate_handler_args(
    step: &str,
    metadata: &crate::HandlerMetadata,
    args: &Value,
) -> Result<(), CruxErr> {
    if args.is_null() && !metadata.args.has_required_args() {
        return Ok(());
    }

    let mut schema = crate::ObjectSchema::new();
    for argument in &metadata.args.args {
        schema = if argument.required {
            schema.required(&argument.name, argument.schema.clone())
        } else {
            schema.optional(&argument.name, argument.schema.clone())
        };
    }
    if metadata.args.allow_extra {
        schema = schema.additional(crate::ValueSchema::Dynamic);
    }
    crate::ValueSchema::object(schema)
        .validate(args)
        .map_err(|violation| {
            CruxErr::step_failed(
                step,
                format!(
                    "handler '{}' arguments contract violated: {violation}",
                    metadata.name
                ),
            )
        })
}

fn validate_handler_output(
    step: &str,
    metadata: &crate::HandlerMetadata,
    output: &crate::HandlerOutput,
) -> Result<(), CruxErr> {
    if let Some(schema) = &metadata.output_schema {
        schema.validate(&output.value).map_err(|violation| {
            CruxErr::step_failed(
                step,
                format!(
                    "handler '{}' output contract violated: {violation}",
                    metadata.name
                ),
            )
        })?;
    }

    match (metadata.confidence, output.confidence) {
        (Some(crate::ConfidenceCapability::Always), None) => Err(CruxErr::step_failed(
            step,
            format!(
                "handler '{}' confidence contract violated: expected handler to always report confidence",
                metadata.name
            ),
        )),
        (Some(crate::ConfidenceCapability::Never), Some(_)) => Err(CruxErr::step_failed(
            step,
            format!(
                "handler '{}' confidence contract violated: expected handler to never report confidence",
                metadata.name
            ),
        )),
        _ => Ok(()),
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

fn budget_from_def(def: &BudgetDef) -> Result<Budget, CruxErr> {
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
        let micros = cost_cents.checked_mul(10_000).ok_or_else(|| {
            CruxErr::step_failed(
                "budget",
                "cost_cents budget is too large to convert exactly to microdollars",
            )
        })?;
        budgets.push(Budget::usd(UsdAmount::from_micros(micros)));
    }
    Ok(match budgets.as_slice() {
        [] => Budget::default(),
        [_] => budgets.into_iter().next().unwrap_or_default(),
        _ => Budget::combined(budgets),
    })
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

    struct DelayedMeter {
        duration: Option<std::time::Duration>,
    }

    impl InvocationMeter for DelayedMeter {
        fn invoke_budgeted_step<'a, F, Fut, T>(
            &'a mut self,
            _name: &'a str,
            f: F,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = BudgetedInvocation<T>> + Send + 'a>>
        where
            F: FnOnce() -> Fut + Send + 'a,
            Fut: std::future::Future<Output = Result<T, CruxErr>> + Send + 'a,
            T: serde::Serialize + serde::de::DeserializeOwned + Send + 'a,
        {
            Box::pin(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                BudgetedInvocation {
                    outcome: f().await,
                    executed: true,
                }
            })
        }

        fn reserve_invocations(&mut self, _count: u64) -> Result<(), CruxErr> {
            Ok(())
        }

        fn record_invocation_usage(
            &mut self,
            _step: &str,
            _usage: HandlerUsage,
            duration: std::time::Duration,
        ) -> Result<(), CruxErr> {
            self.duration = Some(duration);
            Ok(())
        }

        fn budget_usage(&self) -> crux_types::budget::BudgetUsage {
            crux_types::budget::BudgetUsage::default()
        }
    }

    #[tokio::test]
    async fn invocation_duration_starts_when_live_handler_closure_begins() {
        let mut meter = DelayedMeter { duration: None };
        let handler: crate::registry::BoxHandler = Arc::new(|_input| {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                crate::HandlerExecution::free(Ok(crate::HandlerOutput::new(Value::Null)))
            })
        });

        run_step_once(&mut meter, "timed", handler, Value::Null, None)
            .await
            .unwrap();

        let duration = meter.duration.expect("duration recorded");
        assert!(duration >= std::time::Duration::from_millis(5));
        assert!(
            duration < std::time::Duration::from_millis(30),
            "{duration:?}"
        );
    }

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
    async fn retry_accounts_failed_and_successful_attempts_exactly_once() {
        let attempts = Arc::new(AtomicU32::new(0));
        let called = attempts.clone();
        let mut registry = HandlerRegistry::new();
        registry.handler_metered("flaky", move |input: Value| {
            let called = called.clone();
            async move {
                let attempt = called.fetch_add(1, Ordering::SeqCst);
                let usage = HandlerUsage::metered(0, UsdAmount::from_micros(1));
                if attempt == 0 {
                    crate::HandlerExecution::failure(
                        CruxErr::step_failed("flaky", "first attempt"),
                        usage,
                    )
                } else {
                    crate::HandlerExecution::success(crate::HandlerOutput::new(input), usage)
                }
            }
        });
        let pipeline = crate::load(
            "pipeline: retry_meter\nbudget:\n  usd: 0.000001\nsteps:\n  - step: flaky\n    retry:\n      count: 1\n      delay_ms: 0\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;
        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded {
                actual_micros: 2,
                ..
            })
        ));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn on_error_accounts_failed_primary_and_fallback_exactly_once() {
        let mut registry = HandlerRegistry::new();
        registry.handler_metered("primary", |_input: Value| async move {
            crate::HandlerExecution::failure(
                CruxErr::step_failed("primary", "failed"),
                HandlerUsage::metered(0, UsdAmount::from_micros(1)),
            )
        });
        registry.handler_metered("fallback", |input: Value| async move {
            crate::HandlerExecution::success(
                crate::HandlerOutput::new(input),
                HandlerUsage::metered(0, UsdAmount::from_micros(1)),
            )
        });
        let pipeline = crate::load(
            "pipeline: fallback_meter\nbudget:\n  usd: 0.000001\nsteps:\n  - step: primary\n    on_error:\n      handler: fallback\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;
        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded {
                actual_micros: 2,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn join_accounts_every_completed_parallel_arm_exactly_once() {
        let mut registry = HandlerRegistry::new();
        for name in ["one", "two"] {
            registry.handler_metered(name, |input: Value| async move {
                crate::HandlerExecution::success(
                    crate::HandlerOutput::new(input),
                    HandlerUsage::metered(0, UsdAmount::from_micros(1)),
                )
            });
        }
        let pipeline = crate::load(
            "pipeline: join_meter\nbudget:\n  usd: 0.000001\nsteps:\n  - join_all: both\n    arms: [one, two]\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;
        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded {
                actual_micros: 2,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn route_accounts_only_the_selected_handler_once() {
        let unselected = Arc::new(AtomicU32::new(0));
        let low_calls = unselected.clone();
        let mut registry = HandlerRegistry::new();
        registry.handler_free("score", |_input: Value| async move {
            Ok(crate::HandlerOutput::with_confidence(Value::Null, 0.9))
        });
        registry.handler_metered("high", |input: Value| async move {
            crate::HandlerExecution::success(
                crate::HandlerOutput::new(input),
                HandlerUsage::metered(0, UsdAmount::from_micros(1)),
            )
        });
        registry.handler_metered("low", move |input: Value| {
            let low_calls = low_calls.clone();
            async move {
                low_calls.fetch_add(1, Ordering::SeqCst);
                crate::HandlerExecution::success(
                    crate::HandlerOutput::new(input),
                    HandlerUsage::metered(0, UsdAmount::from_micros(1)),
                )
            }
        });
        let pipeline = crate::load(
            "pipeline: route_meter\nbudget:\n  usd: 0\nsteps:\n  - step: score\n  - route_on_confidence: route\n    value: \"{{ steps.score.confidence }}\"\n    routes:\n      - range: \"[0.0, 0.5)\"\n        label: low\n        handler: low\n      - range: \"[0.5, 1.0]\"\n        label: high\n        handler: high\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;
        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded {
                actual_micros: 1,
                ..
            })
        ));
        assert_eq!(unselected.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn selected_route_reserves_step_before_handler_invocation() {
        let selected_calls = Arc::new(AtomicU32::new(0));
        let calls = selected_calls.clone();
        let mut registry = HandlerRegistry::new();
        registry.handler_free("score", |_input: Value| async move {
            Ok(crate::HandlerOutput::with_confidence(Value::Null, 0.9))
        });
        registry.handler_value_free("selected", move |input: Value| {
            let calls = calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(input)
            }
        });
        let pipeline = crate::load(
            "pipeline: route_steps\nbudget: { steps: 1 }\nsteps:\n  - step: score\n  - route_on_confidence: route\n    value: \"{{ steps.score.confidence }}\"\n    routes:\n      - range: \"[0.0, 1.0]\"\n        label: selected\n        handler: selected\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;

        assert!(matches!(
            result.value(),
            Err(CruxErr::StepBudgetExceeded { .. })
        ));
        assert_eq!(selected_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_paid_join_is_metered_exactly_once() {
        let mut registry = HandlerRegistry::new();
        registry.handler_metered("paid_failure", |_input: Value| async move {
            crate::HandlerExecution::failure(
                CruxErr::step_failed("paid_failure", "provider failed"),
                HandlerUsage::metered(4, UsdAmount::from_micros(1)),
            )
        });
        let pipeline = crate::load(
            "pipeline: failed_join\nbudget: { usd: 0 }\nsteps:\n  - join_all: work\n    arms: [paid_failure]\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;
        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded {
                actual_micros: 1,
                source: Some(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn failed_paid_route_is_metered_exactly_once() {
        let mut registry = HandlerRegistry::new();
        registry.handler_free("score", |_input: Value| async move {
            Ok(crate::HandlerOutput::with_confidence(Value::Null, 0.9))
        });
        registry.handler_metered("paid_failure", |_input: Value| async move {
            crate::HandlerExecution::failure(
                CruxErr::step_failed("paid_failure", "provider failed"),
                HandlerUsage::metered(4, UsdAmount::from_micros(1)),
            )
        });
        let pipeline = crate::load(
            "pipeline: failed_route\nbudget: { usd: 0 }\nsteps:\n  - step: score\n  - route_on_confidence: route\n    value: \"{{ steps.score.confidence }}\"\n    routes:\n      - range: \"[0.0, 1.0]\"\n        label: selected\n        handler: paid_failure\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;
        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded {
                actual_micros: 1,
                source: Some(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn failed_paid_speculation_is_metered_exactly_once() {
        let mut registry = HandlerRegistry::new();
        registry.handler_metered("paid_failure", |_input: Value| async move {
            crate::HandlerExecution::failure(
                CruxErr::step_failed("paid_failure", "provider failed"),
                HandlerUsage::metered(4, UsdAmount::from_micros(1)),
            )
        });
        let pipeline = crate::load(
            "pipeline: failed_speculation\nbudget: { usd: 0 }\nsteps:\n  - speculate: work\n    mode: first_ok\n    arms: [paid_failure]\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;
        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded {
                actual_micros: 1,
                source: Some(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn legacy_budget_conversion_succeeds_for_pipeline_and_target() {
        let mut registry = HandlerRegistry::new();
        registry.handler_metered("paid", |input: Value| async move {
            crate::HandlerExecution::success(
                crate::HandlerOutput::new(input),
                HandlerUsage::metered(0, UsdAmount::from_micros(10_000)),
            )
        });
        let runner = Runner::new(Arc::new(registry));
        let pipeline = crate::load(
            "pipeline: legacy\nbudget:\n  calls: 1\n  cost_cents: 1\nsteps:\n  - step: paid\n",
        )
        .unwrap();
        assert!(runner.run(&pipeline, Value::Null).await.value().is_ok());

        let cruxfile = crate::load_cruxfile(
            "project: legacy\ndefault: check\ntargets:\n  check:\n    budget:\n      calls: 1\n      cost_cents: 1\n    steps:\n      - step: paid\n",
        )
        .unwrap();
        let target = cruxfile.targets.get("check").unwrap();
        assert!(
            runner
                .run_target(target, "check", None)
                .await
                .value()
                .is_ok()
        );
    }

    #[tokio::test]
    async fn run_target_rejects_cost_cents_overflow() {
        let overflow = u64::MAX / 10_000 + 1;
        let cruxfile = crate::load_cruxfile(&format!(
            "project: overflow\ndefault: check\ntargets:\n  check:\n    budget:\n      cost_cents: {overflow}\n    steps: []\n"
        ))
        .unwrap();
        let target = cruxfile.targets.get("check").unwrap();

        let result = Runner::new(Arc::new(HandlerRegistry::new()))
            .run_target(target, "check", None)
            .await;

        assert!(result.value().is_err());
        assert!(
            result
                .value()
                .unwrap_err()
                .to_string()
                .contains("cost_cents")
        );
    }

    #[tokio::test]
    async fn sequential_pipe_stops_after_first_over_budget_invocation() {
        let later_calls = Arc::new(AtomicU32::new(0));
        let later = later_calls.clone();
        let mut registry = HandlerRegistry::new();
        registry.handler_metered("paid", |input: Value| async move {
            crate::HandlerExecution::success(
                crate::HandlerOutput::new(input),
                HandlerUsage::metered(0, UsdAmount::from_micros(2)),
            )
        });
        registry.handler_metered("later", move |input: Value| {
            let later = later.clone();
            async move {
                later.fetch_add(1, Ordering::SeqCst);
                crate::HandlerExecution::success(
                    crate::HandlerOutput::new(input),
                    HandlerUsage::metered(0, UsdAmount::from_micros(1)),
                )
            }
        });
        let pipeline = crate::load(
            "pipeline: pipe_budget\nbudget:\n  usd: 0.000001\nsteps:\n  - pipe: sequence\n    stages: [paid, later]\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;

        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded { .. })
        ));
        assert_eq!(later_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sequential_speculation_stops_after_first_over_budget_invocation() {
        let later_calls = Arc::new(AtomicU32::new(0));
        let later = later_calls.clone();
        let mut registry = HandlerRegistry::new();
        registry.handler_metered("paid", |_input: Value| async move {
            crate::HandlerExecution::success(
                crate::HandlerOutput::new(serde_json::json!({"score": 1})),
                HandlerUsage::metered(0, UsdAmount::from_micros(2)),
            )
        });
        registry.handler_metered("later", move |_input: Value| {
            let later = later.clone();
            async move {
                later.fetch_add(1, Ordering::SeqCst);
                crate::HandlerExecution::success(
                    crate::HandlerOutput::new(serde_json::json!({"score": 2})),
                    HandlerUsage::metered(0, UsdAmount::from_micros(1)),
                )
            }
        });
        let pipeline = crate::load(
            "pipeline: speculate_budget\nbudget:\n  usd: 0.000001\nsteps:\n  - speculate: choices\n    mode: pick_best\n    arms: [paid, later]\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(registry))
            .run(&pipeline, Value::Null)
            .await;

        assert!(matches!(
            result.value(),
            Err(CruxErr::UsdBudgetExceeded { .. })
        ));
        assert_eq!(later_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn accounting_retains_all_dimensions_and_completed_reports_before_violation() {
        let mut ctx = CruxCtx::new("meter");
        ctx.set_budget(Budget::combined(vec![
            Budget::duration(std::time::Duration::ZERO),
            Budget::tokens(0),
            Budget::usd(UsdAmount::ZERO),
        ]));
        let cells = vec![
            Arc::new(Mutex::new(Some((
                HandlerUsage::metered(2, UsdAmount::from_micros(3)),
                std::time::Duration::from_millis(1),
            )))),
            Arc::new(Mutex::new(Some((
                HandlerUsage::metered(5, UsdAmount::from_micros(7)),
                std::time::Duration::from_millis(2),
            )))),
        ];

        let error = record_usage_cells(&mut ctx, ["a", "b"].into_iter(), &cells, None)
            .expect_err("completed usage must still report a violation");

        assert!(error.to_string().to_lowercase().contains("duration"));
        let usage = ctx.budget_usage();
        assert_eq!(usage.duration_ms, 3);
        assert_eq!(usage.tokens, 7);
        assert_eq!(usage.usd.map(UsdAmount::micros), Some(10));
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
    async fn replayed_step_under_usd_budget_consumes_no_usage() {
        let counter = Arc::new(AtomicU32::new(0));
        let called = counter.clone();
        let mut reg = HandlerRegistry::new();
        reg.handler_value_free("test::free", move |input: Value| {
            let called = called.clone();
            async move {
                called.fetch_add(1, Ordering::SeqCst);
                Ok(input)
            }
        });
        let pipeline = crate::load(
            "pipeline: replay_budget\nbudget:\n  usd: 0\nsteps:\n  - step: free\n    handler: test::free\n",
        )
        .unwrap();
        let runner = Runner::new(Arc::new(reg));

        let first = runner.run(&pipeline, serde_json::json!({})).await;
        assert!(first.value().is_ok());
        let replayed = runner
            .run_with_replay(&pipeline, serde_json::json!({}), &first, ReplayMode::Strict)
            .await;

        assert!(replayed.value().is_ok(), "{:?}", replayed.value());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(replayed.steps[0].duration_ms, 0);
    }

    #[tokio::test]
    async fn pre_execution_step_rejection_preserves_error_and_records_no_duration() {
        let mut reg = HandlerRegistry::new();
        reg.handler_value_free("test::free", |input: Value| async move { Ok(input) });
        let pipeline = crate::load(
            "pipeline: rejected\nbudget:\n  steps: 0\n  usd: 0\nsteps:\n  - step: blocked\n    handler: test::free\n",
        )
        .unwrap();

        let result = Runner::new(Arc::new(reg))
            .run(&pipeline, serde_json::json!({}))
            .await;

        assert!(matches!(
            result.value(),
            Err(CruxErr::StepBudgetExceeded {
                limit: 0,
                attempted: 1
            })
        ));
        assert!(result.steps.is_empty());
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
