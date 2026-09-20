//! Typed pipeline compilation options and results.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use crate::expr::ExprError;
use crate::ir::{
    BindingId, TypedArm, TypedBinding, TypedHandlerStep, TypedLoopBinding, TypedLoopBindings,
    TypedPipeline, TypedRouteBranch, TypedStep, TypedStepKind, TypedValue,
};
use crate::metadata::{ConfidenceCapability, ValueSchema};
use crate::registry::HandlerRegistry;
use crate::schema::{
    ArmDef, ForEachNode, JoinAllNode, PipeNode, PipelineDef, PollNode, RepeatNode, RouteBranch,
    RouteNode, SpeculateMode, SpeculateNode, StepDef, StepNode, WhileNode,
};
use crate::validator::{DiagnosticSeverity, ValidationCode, ValidationDiagnostic};

#[derive(Clone)]
struct StepBinding {
    index: usize,
    output: ValueSchema,
    confidence: ConfidenceCapability,
}

struct ReferenceScope<'a> {
    input_schema: Option<&'a ValueSchema>,
    variables: &'a BTreeMap<String, TypedBinding>,
    variable_positions: &'a HashMap<String, usize>,
    current_variable: Option<(&'a str, usize)>,
    steps: &'a BTreeMap<String, StepBinding>,
    step_positions: &'a HashMap<String, usize>,
    current_step: Option<usize>,
    loop_bindings: &'a [TypedLoopBindings],
    options: CompileOptions,
    diagnostic_sink: Option<(&'a RefCell<Vec<ValidationDiagnostic>>, &'a str)>,
}

struct StepCompileContext<'a> {
    definition: &'a PipelineDef,
    variables: &'a BTreeMap<String, TypedBinding>,
    variable_positions: &'a HashMap<String, usize>,
    step_bindings: &'a BTreeMap<String, StepBinding>,
    step_positions: &'a HashMap<String, usize>,
    index: usize,
    loop_bindings: &'a [TypedLoopBindings],
    binding_ids: &'a Cell<usize>,
    registry: &'a HandlerRegistry,
    options: CompileOptions,
}

impl<'a> StepCompileContext<'a> {
    fn reference_scope<'scope>(
        &'scope self,
        diagnostics: &'scope RefCell<Vec<ValidationDiagnostic>>,
        location: &'scope str,
    ) -> ReferenceScope<'scope> {
        ReferenceScope {
            input_schema: self.definition.input_schema.as_ref(),
            variables: self.variables,
            variable_positions: self.variable_positions,
            current_variable: None,
            steps: self.step_bindings,
            step_positions: self.step_positions,
            current_step: Some(self.index),
            loop_bindings: self.loop_bindings,
            options: self.options,
            diagnostic_sink: Some((diagnostics, location)),
        }
    }
}

/// Static validation strictness used while compiling pipeline definitions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    /// Preserve dynamic extension points as warnings where execution remains safe.
    #[default]
    Permissive,
    /// Require complete contracts and reject dynamic boundaries.
    Strict,
}

/// Options controlling typed pipeline compilation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileOptions {
    mode: CompileMode,
}

impl CompileOptions {
    /// Create permissive compilation options.
    pub const fn permissive() -> Self {
        Self {
            mode: CompileMode::Permissive,
        }
    }

    /// Create strict compilation options.
    pub const fn strict() -> Self {
        Self {
            mode: CompileMode::Strict,
        }
    }

    /// Return the selected compile mode.
    pub const fn mode(self) -> CompileMode {
        self.mode
    }

    /// Return the severity assigned to one diagnostic code in this mode.
    pub const fn severity_for(self, code: ValidationCode) -> DiagnosticSeverity {
        match (self.mode, code) {
            (
                CompileMode::Permissive,
                ValidationCode::UnknownHandler
                | ValidationCode::UnknownAgent
                | ValidationCode::MissingContract
                | ValidationCode::MissingInputSchema
                | ValidationCode::DynamicBoundary,
            ) => DiagnosticSeverity::Warning,
            _ => DiagnosticSeverity::Error,
        }
    }
}

/// Compiler output containing diagnostics and an optional executable artifact.
#[derive(Debug, Clone)]
pub struct Compilation<T> {
    artifact: Option<T>,
    diagnostics: Vec<ValidationDiagnostic>,
}

impl<T> Compilation<T> {
    /// Build a compilation result, discarding the artifact when any error exists.
    pub fn new(artifact: Option<T>, diagnostics: Vec<ValidationDiagnostic>) -> Self {
        let has_errors = diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
        Self {
            artifact: if has_errors { None } else { artifact },
            diagnostics,
        }
    }

    /// Return the compiled artifact when this result is executable.
    pub fn artifact(&self) -> Option<&T> {
        self.artifact.as_ref()
    }

    /// Return all compiler diagnostics.
    pub fn diagnostics(&self) -> &[ValidationDiagnostic] {
        &self.diagnostics
    }

    /// Count error diagnostics.
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            .count()
    }

    /// Count warning diagnostics.
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .count()
    }

    /// Return whether compilation produced no errors.
    pub fn is_ok(&self) -> bool {
        self.error_count() == 0
    }

    /// Return whether an artifact is available for execution.
    pub fn is_executable(&self) -> bool {
        self.is_ok() && self.artifact.is_some()
    }

    /// Consume the result and return its artifact.
    pub fn into_artifact(self) -> Option<T> {
        self.artifact
    }

    /// Consume the result into its artifact and diagnostics.
    pub fn into_parts(self) -> (Option<T>, Vec<ValidationDiagnostic>) {
        (self.artifact, self.diagnostics)
    }
}

/// Compile simple handler steps into resolved runtime-only typed IR.
pub fn compile_pipeline(
    definition: &PipelineDef,
    registry: &HandlerRegistry,
    options: CompileOptions,
) -> Compilation<TypedPipeline> {
    let mut diagnostics = Vec::new();
    let mut steps = Vec::with_capacity(definition.steps.len());
    let mut unresolved = false;

    if definition.input_schema.is_none() && options.mode() == CompileMode::Strict {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::MissingInputSchema,
            "input_schema",
            "strict compilation requires an input schema",
        ));
        unresolved = true;
    }

    if let Some(schema) = &definition.input_schema
        && let Err(error) = schema.validate_definition()
    {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::TypeMismatch,
            "input_schema",
            error.to_string(),
        ));
        unresolved = true;
    }

    let variables = compile_variables(definition, &mut diagnostics, &mut unresolved);
    let step_positions = definition
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| step_name(step).map(|name| (name.to_string(), index)))
        .collect::<HashMap<_, _>>();
    let mut step_bindings = BTreeMap::new();
    let mut current_schema = definition
        .input_schema
        .clone()
        .unwrap_or(ValueSchema::Dynamic);
    let empty_variable_positions = HashMap::new();
    let loop_bindings = Vec::new();
    let binding_ids = Cell::new(variables.len());

    for (index, step) in definition.steps.iter().enumerate() {
        let location = format!("steps[{index}]");
        let Some(name) = step_name(step) else {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::InvalidControlFlow,
                &location,
                "typed compilation for this combinator is not implemented",
            ));
            unresolved = true;
            continue;
        };
        if step_bindings.contains_key(name) {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::DuplicateName,
                &location,
                format!("step '{name}' is declared more than once"),
            ));
            unresolved = true;
            continue;
        }

        let context = StepCompileContext {
            definition,
            variables: &variables,
            variable_positions: &empty_variable_positions,
            step_bindings: &step_bindings,
            step_positions: &step_positions,
            index,
            loop_bindings: &loop_bindings,
            binding_ids: &binding_ids,
            registry,
            options,
        };
        let compiled = match step {
            StepDef::Step(node) => {
                compile_handler_step(node, &location, &context, &mut diagnostics, &mut unresolved)
            }
            StepDef::Pipe(node) => compile_pipe_step(
                node,
                &location,
                &current_schema,
                &context,
                &mut diagnostics,
                &mut unresolved,
            ),
            StepDef::JoinAll(node) => compile_join_step(
                node,
                &location,
                &current_schema,
                &context,
                &mut diagnostics,
                &mut unresolved,
            ),
            StepDef::RouteOnConfidence(node) => compile_route_step(
                node,
                &location,
                &current_schema,
                &context,
                &mut diagnostics,
                &mut unresolved,
            ),
            StepDef::Speculate(node) => compile_speculate_step(
                node,
                &location,
                &current_schema,
                &context,
                &mut diagnostics,
                &mut unresolved,
            ),
            StepDef::ForEach(node) => compile_for_each_step(
                node,
                &location,
                &current_schema,
                &context,
                &mut diagnostics,
                &mut unresolved,
            ),
            StepDef::While(node) => compile_while_step(
                node,
                &location,
                &current_schema,
                &context,
                &mut diagnostics,
                &mut unresolved,
            ),
            StepDef::Repeat(node) => compile_repeat_step(
                node,
                &location,
                &current_schema,
                &context,
                &mut diagnostics,
                &mut unresolved,
            ),
            StepDef::Poll(node) => compile_poll_step(
                node,
                &location,
                &current_schema,
                &context,
                &mut diagnostics,
                &mut unresolved,
            ),
            StepDef::Delegate(_) => None,
        };

        if let Some(compiled) = compiled {
            current_schema = compiled.output_schema.clone();
            step_bindings.insert(
                compiled.name.clone(),
                StepBinding {
                    index,
                    output: compiled.output_schema.clone(),
                    confidence: compiled.confidence,
                },
            );
            steps.push(compiled);
        }
    }

    let artifact = (!unresolved).then(|| TypedPipeline::new(definition, variables, steps));
    Compilation::new(artifact, diagnostics)
}

fn step_name(step: &StepDef) -> Option<&str> {
    match step {
        StepDef::Step(node) => Some(&node.step),
        StepDef::Pipe(node) => Some(&node.pipe),
        StepDef::JoinAll(node) => Some(&node.join_all),
        StepDef::RouteOnConfidence(node) => Some(&node.route_on_confidence),
        StepDef::Speculate(node) => Some(&node.speculate),
        StepDef::Poll(node) => Some(&node.poll),
        StepDef::ForEach(node) => Some(node.label()),
        StepDef::While(node) => Some(&node.r#while),
        StepDef::Repeat(node) => Some(&node.repeat),
        StepDef::Delegate(_) => None,
    }
}

fn compile_handler_step(
    node: &StepNode,
    location: &str,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    let handler_name = node.handler.as_deref().unwrap_or(&node.step);
    let runner = resolve_runner(handler_name, location, context, diagnostics, unresolved)?;
    let args = compile_args(
        node.args.as_ref(),
        location,
        context,
        diagnostics,
        unresolved,
    )?;
    let output_schema = runner
        .metadata()
        .output_schema
        .clone()
        .unwrap_or(ValueSchema::Dynamic);
    let confidence = runner
        .metadata()
        .confidence
        .unwrap_or(ConfidenceCapability::Optional);

    Some(TypedStep {
        name: node.step.clone(),
        kind: TypedStepKind::Handler(Box::new(TypedHandlerStep {
            node: node.clone(),
            runner,
            args,
        })),
        output_schema,
        confidence,
    })
}

fn compile_pipe_step(
    node: &PipeNode,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    if node.stages.is_empty() {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::InvalidControlFlow,
            location,
            "pipe must contain at least one stage",
        ));
        *unresolved = true;
        return None;
    }

    let mut stages = Vec::with_capacity(node.stages.len());
    let mut stage_input = input_schema.clone();
    for (stage_index, stage) in node.stages.iter().enumerate() {
        let stage_location = format!("{location}.stages[{stage_index}]");
        let typed = compile_arm(
            stage,
            &stage_location,
            &stage_input,
            context,
            diagnostics,
            unresolved,
        )?;
        stage_input = typed.output_schema.clone();
        stages.push(typed);
    }
    let confidence = stages
        .last()
        .map(|stage| stage.confidence)
        .unwrap_or(ConfidenceCapability::Never);

    Some(TypedStep {
        name: node.pipe.clone(),
        kind: TypedStepKind::Pipe {
            node: node.clone(),
            stages,
        },
        output_schema: stage_input,
        confidence,
    })
}

fn compile_join_step(
    node: &JoinAllNode,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    if node.arms.is_empty() {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::InvalidControlFlow,
            location,
            "join_all must contain at least one arm",
        ));
        *unresolved = true;
        return None;
    }

    let mut arms = Vec::with_capacity(node.arms.len());
    for (arm_index, arm) in node.arms.iter().enumerate() {
        let arm_location = format!("{location}.arms[{arm_index}]");
        arms.push(compile_arm(
            arm,
            &arm_location,
            input_schema,
            context,
            diagnostics,
            unresolved,
        )?);
    }
    let item_schema = ValueSchema::union(arms.iter().map(|arm| arm.output_schema.clone()))
        .unwrap_or(ValueSchema::Dynamic);
    let confidence = join_confidence(&arms);

    Some(TypedStep {
        name: node.join_all.clone(),
        kind: TypedStepKind::JoinAll {
            node: node.clone(),
            arms,
        },
        output_schema: ValueSchema::array(item_schema),
        confidence,
    })
}

fn compile_route_step(
    node: &RouteNode,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    validate_route_ranges(node, location, diagnostics, unresolved);

    let expression_diagnostics = RefCell::new(Vec::new());
    let scope = context.reference_scope(&expression_diagnostics, location);
    let resolve = |path: &str| resolve_reference_schema(path, &scope);
    let value =
        match TypedValue::compile_with(&serde_json::Value::String(node.value.clone()), &resolve) {
            Ok(value) => value,
            Err(error) => {
                diagnostics.push(ValidationDiagnostic::error_with_code(
                    validation_code_for_expression(&error),
                    format!("{location}.value"),
                    error.to_string(),
                ));
                *unresolved = true;
                return None;
            }
        };
    diagnostics.extend(expression_diagnostics.into_inner());
    check_numeric_schema(
        &value.schema,
        format!("{location}.value"),
        "route confidence",
        context.options,
        diagnostics,
        unresolved,
    );

    let mut branches = Vec::with_capacity(node.routes.len());
    for (branch_index, branch) in node.routes.iter().enumerate() {
        branches.push(compile_route_branch(
            branch,
            &format!("{location}.routes[{branch_index}]"),
            input_schema,
            context,
            diagnostics,
            unresolved,
        )?);
    }
    let output_schema =
        ValueSchema::union(branches.iter().map(|branch| branch.output_schema.clone()))
            .unwrap_or(ValueSchema::Dynamic);

    Some(TypedStep {
        name: node.route_on_confidence.clone(),
        kind: TypedStepKind::RouteOnConfidence {
            node: node.clone(),
            value,
            branches,
        },
        output_schema,
        confidence: ConfidenceCapability::Always,
    })
}

fn compile_route_branch(
    branch: &RouteBranch,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedRouteBranch> {
    let runner = resolve_runner(&branch.handler, location, context, diagnostics, unresolved)?;
    let expected_input = runner
        .metadata()
        .input_schema
        .as_ref()
        .unwrap_or(&ValueSchema::Dynamic);
    if !expected_input.is_assignable_from(input_schema) {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::TypeMismatch,
            format!("{location}.input"),
            format!(
                "handler '{}' expects {expected_input}, but receives {input_schema}",
                branch.handler
            ),
        ));
        *unresolved = true;
        return None;
    }
    let args = compile_args(
        branch.args.as_ref(),
        location,
        context,
        diagnostics,
        unresolved,
    )?;
    let output_schema = runner
        .metadata()
        .output_schema
        .clone()
        .unwrap_or(ValueSchema::Dynamic);
    let confidence = runner
        .metadata()
        .confidence
        .unwrap_or(ConfidenceCapability::Optional);

    Some(TypedRouteBranch {
        node: branch.clone(),
        runner,
        args,
        output_schema,
        confidence,
    })
}

fn compile_speculate_step(
    node: &SpeculateNode,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    if node.arms.is_empty() {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::InvalidControlFlow,
            location,
            "speculate must contain at least one arm",
        ));
        *unresolved = true;
        return None;
    }

    let mut arms = Vec::with_capacity(node.arms.len());
    for (arm_index, arm) in node.arms.iter().enumerate() {
        let arm_location = format!("{location}.arms[{arm_index}]");
        let typed = compile_arm(
            arm,
            &arm_location,
            input_schema,
            context,
            diagnostics,
            unresolved,
        )?;
        if matches!(node.mode, SpeculateMode::PickBest) {
            check_pick_best_score(
                &typed.output_schema,
                &arm_location,
                context.options,
                diagnostics,
                unresolved,
            );
        }
        arms.push(typed);
    }
    let output_schema = ValueSchema::union(arms.iter().map(|arm| arm.output_schema.clone()))
        .unwrap_or(ValueSchema::Dynamic);

    Some(TypedStep {
        name: node.speculate.clone(),
        kind: TypedStepKind::Speculate {
            node: node.clone(),
            arms,
        },
        output_schema,
        confidence: ConfidenceCapability::Never,
    })
}

fn compile_for_each_step(
    node: &ForEachNode,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    let items = compile_loop_expression(
        &node.items,
        &format!("{location}.items"),
        context,
        diagnostics,
        unresolved,
    )?;
    let item_schema = match &items.schema {
        ValueSchema::Array { items } => items.as_ref().clone(),
        ValueSchema::Dynamic => {
            diagnostics.push(diagnostic_for_mode(
                context.options,
                ValidationCode::DynamicBoundary,
                format!("{location}.items"),
                "for_each items must be an array, but its schema is dynamic",
            ));
            if context.options.mode() == CompileMode::Strict {
                *unresolved = true;
                return None;
            }
            ValueSchema::Dynamic
        }
        schema => {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::TypeMismatch,
                format!("{location}.items"),
                format!("for_each items must be an array, but has schema {schema}"),
            ));
            *unresolved = true;
            return None;
        }
    };
    let bindings = allocate_loop_bindings(
        context.binding_ids,
        Some((node.binding().to_string(), item_schema)),
    );
    let mut frames = context.loop_bindings.to_vec();
    frames.push(bindings.clone());
    let body = compile_nested_steps(
        &node.steps,
        &format!("{location}.steps"),
        input_schema,
        context,
        &frames,
        diagnostics,
        unresolved,
    )?;
    let body_context = body.context(context, &frames);
    let break_if = compile_optional_boolean_expression(
        node.break_if.as_deref(),
        &format!("{location}.break_if"),
        "for_each break_if",
        &body_context,
        diagnostics,
        unresolved,
    )?;
    let output_schema = ValueSchema::union([input_schema.clone(), body.output_schema.clone()])
        .unwrap_or(ValueSchema::Dynamic);

    Some(TypedStep {
        name: node.label().to_string(),
        kind: TypedStepKind::ForEach {
            node: node.clone(),
            items,
            bindings,
            body: body.steps,
            break_if,
        },
        output_schema,
        confidence: ConfidenceCapability::Never,
    })
}

fn compile_while_step(
    node: &WhileNode,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    let condition = compile_boolean_expression(
        &node.condition,
        &format!("{location}.condition"),
        "while condition",
        context,
        diagnostics,
        unresolved,
    )?;
    let bindings = allocate_loop_bindings(context.binding_ids, None);
    let mut frames = context.loop_bindings.to_vec();
    frames.push(bindings.clone());
    let body = compile_nested_steps(
        &node.steps,
        &format!("{location}.steps"),
        input_schema,
        context,
        &frames,
        diagnostics,
        unresolved,
    )?;
    let body_context = body.context(context, &frames);
    let break_if = compile_optional_boolean_expression(
        node.break_if.as_deref(),
        &format!("{location}.break_if"),
        "while break_if",
        &body_context,
        diagnostics,
        unresolved,
    )?;
    let output_schema = ValueSchema::union([input_schema.clone(), body.output_schema.clone()])
        .unwrap_or(ValueSchema::Dynamic);

    Some(TypedStep {
        name: node.r#while.clone(),
        kind: TypedStepKind::While {
            node: node.clone(),
            bindings,
            condition,
            body: body.steps,
            break_if,
        },
        output_schema,
        confidence: ConfidenceCapability::Never,
    })
}

fn compile_repeat_step(
    node: &RepeatNode,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    let bindings = allocate_loop_bindings(context.binding_ids, None);
    let mut frames = context.loop_bindings.to_vec();
    frames.push(bindings.clone());
    let body = compile_nested_steps(
        &node.steps,
        &format!("{location}.steps"),
        input_schema,
        context,
        &frames,
        diagnostics,
        unresolved,
    )?;
    let body_context = body.context(context, &frames);
    let break_if = compile_optional_boolean_expression(
        node.break_if.as_deref(),
        &format!("{location}.break_if"),
        "repeat break_if",
        &body_context,
        diagnostics,
        unresolved,
    )?;
    let output_schema = ValueSchema::union([input_schema.clone(), body.output_schema.clone()])
        .unwrap_or(ValueSchema::Dynamic);

    Some(TypedStep {
        name: node.repeat.clone(),
        kind: TypedStepKind::Repeat {
            node: node.clone(),
            bindings,
            body: body.steps,
            break_if,
        },
        output_schema,
        confidence: ConfidenceCapability::Never,
    })
}

fn compile_poll_step(
    node: &PollNode,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedStep> {
    let bindings = allocate_loop_bindings(context.binding_ids, None);
    let mut frames = context.loop_bindings.to_vec();
    frames.push(bindings.clone());
    let body = compile_nested_steps(
        &node.steps,
        &format!("{location}.steps"),
        input_schema,
        context,
        &frames,
        diagnostics,
        unresolved,
    )?;
    let body_context = body.context(context, &frames);
    let until = compile_boolean_expression(
        &node.until,
        &format!("{location}.until"),
        "poll until",
        &body_context,
        diagnostics,
        unresolved,
    )?;

    Some(TypedStep {
        name: node.poll.clone(),
        kind: TypedStepKind::Poll {
            node: node.clone(),
            bindings,
            body: body.steps,
            until,
        },
        output_schema: body.output_schema,
        confidence: ConfidenceCapability::Never,
    })
}

struct CompiledBody {
    steps: Vec<TypedStep>,
    bindings: BTreeMap<String, StepBinding>,
    positions: HashMap<String, usize>,
    next_index: usize,
    output_schema: ValueSchema,
}

impl CompiledBody {
    fn context<'a>(
        &'a self,
        parent: &'a StepCompileContext<'a>,
        loop_bindings: &'a [TypedLoopBindings],
    ) -> StepCompileContext<'a> {
        StepCompileContext {
            definition: parent.definition,
            variables: parent.variables,
            variable_positions: parent.variable_positions,
            step_bindings: &self.bindings,
            step_positions: &self.positions,
            index: self.next_index,
            loop_bindings,
            binding_ids: parent.binding_ids,
            registry: parent.registry,
            options: parent.options,
        }
    }
}

fn compile_nested_steps(
    definitions: &[StepDef],
    location: &str,
    input_schema: &ValueSchema,
    parent: &StepCompileContext<'_>,
    loop_bindings: &[TypedLoopBindings],
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<CompiledBody> {
    let base_index = parent
        .step_positions
        .values()
        .copied()
        .max()
        .unwrap_or(parent.index)
        + 1;
    let mut positions = parent.step_positions.clone();
    for (offset, step) in definitions.iter().enumerate() {
        if let Some(name) = step_name(step) {
            positions.insert(name.to_string(), base_index + offset);
        }
    }
    let mut bindings = parent.step_bindings.clone();
    let mut local_names = std::collections::HashSet::new();
    let mut steps = Vec::with_capacity(definitions.len());
    let mut current_schema = input_schema.clone();

    for (offset, step) in definitions.iter().enumerate() {
        let step_location = format!("{location}[{offset}]");
        let Some(name) = step_name(step) else {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::InvalidControlFlow,
                &step_location,
                "typed compilation for this combinator is not implemented",
            ));
            *unresolved = true;
            return None;
        };
        if !local_names.insert(name.to_string()) {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::DuplicateName,
                &step_location,
                format!("step '{name}' is declared more than once in this loop body"),
            ));
            *unresolved = true;
            return None;
        }
        let context = StepCompileContext {
            definition: parent.definition,
            variables: parent.variables,
            variable_positions: parent.variable_positions,
            step_bindings: &bindings,
            step_positions: &positions,
            index: base_index + offset,
            loop_bindings,
            binding_ids: parent.binding_ids,
            registry: parent.registry,
            options: parent.options,
        };
        let typed = match step {
            StepDef::Step(node) => {
                compile_handler_step(node, &step_location, &context, diagnostics, unresolved)
            }
            StepDef::Pipe(node) => compile_pipe_step(
                node,
                &step_location,
                &current_schema,
                &context,
                diagnostics,
                unresolved,
            ),
            StepDef::JoinAll(node) => compile_join_step(
                node,
                &step_location,
                &current_schema,
                &context,
                diagnostics,
                unresolved,
            ),
            StepDef::RouteOnConfidence(node) => compile_route_step(
                node,
                &step_location,
                &current_schema,
                &context,
                diagnostics,
                unresolved,
            ),
            StepDef::Speculate(node) => compile_speculate_step(
                node,
                &step_location,
                &current_schema,
                &context,
                diagnostics,
                unresolved,
            ),
            StepDef::ForEach(node) => compile_for_each_step(
                node,
                &step_location,
                &current_schema,
                &context,
                diagnostics,
                unresolved,
            ),
            StepDef::While(node) => compile_while_step(
                node,
                &step_location,
                &current_schema,
                &context,
                diagnostics,
                unresolved,
            ),
            StepDef::Repeat(node) => compile_repeat_step(
                node,
                &step_location,
                &current_schema,
                &context,
                diagnostics,
                unresolved,
            ),
            StepDef::Poll(node) => compile_poll_step(
                node,
                &step_location,
                &current_schema,
                &context,
                diagnostics,
                unresolved,
            ),
            StepDef::Delegate(_) => None,
        }?;
        current_schema = typed.output_schema.clone();
        bindings.insert(
            typed.name.clone(),
            StepBinding {
                index: base_index + offset,
                output: typed.output_schema.clone(),
                confidence: typed.confidence,
            },
        );
        steps.push(typed);
    }

    Some(CompiledBody {
        steps,
        bindings,
        positions,
        next_index: base_index + definitions.len(),
        output_schema: current_schema,
    })
}

fn allocate_loop_bindings(
    binding_ids: &Cell<usize>,
    item: Option<(String, ValueSchema)>,
) -> TypedLoopBindings {
    let index = allocate_loop_binding(binding_ids, "index".to_string(), ValueSchema::Integer);
    let item = item.map(|(name, schema)| allocate_loop_binding(binding_ids, name, schema));
    TypedLoopBindings { index, item }
}

fn allocate_loop_binding(
    binding_ids: &Cell<usize>,
    name: String,
    schema: ValueSchema,
) -> TypedLoopBinding {
    let id = binding_ids.get();
    binding_ids.set(id + 1);
    TypedLoopBinding {
        id: BindingId(id),
        name,
        schema,
    }
}

fn compile_loop_expression(
    expression: &str,
    location: &str,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedValue> {
    let expression_diagnostics = RefCell::new(Vec::new());
    let scope = context.reference_scope(&expression_diagnostics, location);
    let resolve = |path: &str| resolve_reference_schema(path, &scope);
    let result =
        TypedValue::compile_with(&serde_json::Value::String(expression.to_string()), &resolve);
    diagnostics.extend(expression_diagnostics.into_inner());
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                validation_code_for_expression(&error),
                location,
                error.to_string(),
            ));
            *unresolved = true;
            None
        }
    }
}

fn compile_boolean_expression(
    expression: &str,
    location: &str,
    subject: &str,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedValue> {
    let value = compile_loop_expression(expression, location, context, diagnostics, unresolved)?;
    check_boolean_schema(
        &value.schema,
        location,
        subject,
        context.options,
        diagnostics,
        unresolved,
    );
    Some(value)
}

fn compile_optional_boolean_expression(
    expression: Option<&str>,
    location: &str,
    subject: &str,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<Option<TypedValue>> {
    match expression {
        Some(expression) => compile_boolean_expression(
            expression,
            location,
            subject,
            context,
            diagnostics,
            unresolved,
        )
        .map(Some),
        None => Some(None),
    }
}

fn check_boolean_schema(
    schema: &ValueSchema,
    location: &str,
    subject: &str,
    options: CompileOptions,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) {
    if ValueSchema::Boolean.is_assignable_from(schema) {
        return;
    }
    if schema_contains_dynamic(schema) {
        diagnostics.push(diagnostic_for_mode(
            options,
            ValidationCode::DynamicBoundary,
            location,
            format!("{subject} must be boolean, but its schema is dynamic"),
        ));
        if options.mode() == CompileMode::Strict {
            *unresolved = true;
        }
    } else {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::TypeMismatch,
            location,
            format!("{subject} must be boolean, but has schema {schema}"),
        ));
        *unresolved = true;
    }
}

#[derive(Clone, Copy)]
struct ParsedRouteRange {
    lower: f64,
    upper: f64,
    includes_lower: bool,
    includes_upper: bool,
}

fn validate_route_ranges(
    node: &RouteNode,
    location: &str,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) {
    let mut ranges = Vec::with_capacity(node.routes.len());
    for (index, branch) in node.routes.iter().enumerate() {
        match parse_route_range(&branch.range) {
            Ok(range)
                if range.lower >= 0.0
                    && range.upper <= 1.0
                    && (range.lower < range.upper
                        || (range.lower == range.upper
                            && range.includes_lower
                            && range.includes_upper)) =>
            {
                ranges.push((index, range));
            }
            Ok(_) => {
                diagnostics.push(ValidationDiagnostic::error_with_code(
                    ValidationCode::InvalidRoute,
                    format!("{location}.routes[{index}].range"),
                    format!(
                        "confidence range '{}' must be non-empty and within [0.0, 1.0]",
                        branch.range
                    ),
                ));
                *unresolved = true;
            }
            Err(message) => {
                diagnostics.push(ValidationDiagnostic::error_with_code(
                    ValidationCode::InvalidRoute,
                    format!("{location}.routes[{index}].range"),
                    format!("invalid confidence range '{}': {message}", branch.range),
                ));
                *unresolved = true;
            }
        }
    }

    ranges.sort_by(|left, right| {
        left.1
            .lower
            .total_cmp(&right.1.lower)
            .then(left.1.upper.total_cmp(&right.1.upper))
    });
    if ranges
        .first()
        .is_none_or(|(_, range)| range.lower != 0.0 || !range.includes_lower)
    {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::InvalidRoute,
            location,
            "confidence routes leave a gap at 0.0",
        ));
        *unresolved = true;
    }
    if ranges
        .last()
        .is_none_or(|(_, range)| range.upper != 1.0 || !range.includes_upper)
    {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::InvalidRoute,
            location,
            "confidence routes leave a gap at 1.0",
        ));
        *unresolved = true;
    }
    for pair in ranges.windows(2) {
        let (left_index, left) = pair[0];
        let (right_index, right) = pair[1];
        let overlaps = left.upper > right.lower
            || (left.upper == right.lower && left.includes_upper && right.includes_lower);
        let has_gap = left.upper < right.lower
            || (left.upper == right.lower && !left.includes_upper && !right.includes_lower);
        if overlaps || has_gap {
            let issue = if overlaps { "overlap" } else { "leave a gap" };
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::InvalidRoute,
                location,
                format!("confidence routes {left_index} and {right_index} {issue}"),
            ));
            *unresolved = true;
        }
    }
}

fn parse_route_range(range: &str) -> Result<ParsedRouteRange, &'static str> {
    let range = range.trim();
    let includes_lower = match range.as_bytes().first() {
        Some(b'[') => true,
        Some(b'(') => false,
        _ => return Err("missing opening bracket"),
    };
    let includes_upper = match range.as_bytes().last() {
        Some(b']') => true,
        Some(b')') => false,
        _ => return Err("missing closing bracket"),
    };
    let inner = range
        .get(1..range.len().saturating_sub(1))
        .ok_or("missing bounds")?;
    let (lower, upper) = inner
        .split_once(',')
        .ok_or("expected lower and upper bounds")?;
    let lower = lower
        .trim()
        .parse::<f64>()
        .map_err(|_| "invalid lower bound")?;
    let upper = upper
        .trim()
        .parse::<f64>()
        .map_err(|_| "invalid upper bound")?;
    if !lower.is_finite() || !upper.is_finite() {
        return Err("bounds must be finite");
    }
    Ok(ParsedRouteRange {
        lower,
        upper,
        includes_lower,
        includes_upper,
    })
}

fn check_numeric_schema(
    schema: &ValueSchema,
    location: String,
    subject: &str,
    options: CompileOptions,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) {
    if ValueSchema::Number.is_assignable_from(schema) {
        return;
    }
    if schema_contains_dynamic(schema) {
        diagnostics.push(diagnostic_for_mode(
            options,
            ValidationCode::DynamicBoundary,
            location,
            format!("{subject} must be numeric, but its schema is dynamic"),
        ));
        if options.mode() == CompileMode::Strict {
            *unresolved = true;
        }
    } else {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::TypeMismatch,
            location,
            format!("{subject} must be numeric, but has schema {schema}"),
        ));
        *unresolved = true;
    }
}

fn check_pick_best_score(
    schema: &ValueSchema,
    location: &str,
    options: CompileOptions,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) {
    match pick_best_score_status(schema) {
        ScoreStatus::Valid => {}
        ScoreStatus::Dynamic => {
            diagnostics.push(diagnostic_for_mode(
                options,
                ValidationCode::DynamicBoundary,
                format!("{location}.output.score"),
                "pick_best arm output must contain a required numeric 'score' property",
            ));
            if options.mode() == CompileMode::Strict {
                *unresolved = true;
            }
        }
        ScoreStatus::Invalid => {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::TypeMismatch,
                format!("{location}.output.score"),
                "pick_best arm output must contain a required numeric 'score' property",
            ));
            *unresolved = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScoreStatus {
    Valid,
    Dynamic,
    Invalid,
}

fn pick_best_score_status(schema: &ValueSchema) -> ScoreStatus {
    match schema {
        ValueSchema::Dynamic => ScoreStatus::Dynamic,
        ValueSchema::Object(object) => match object.property("score") {
            Some(property)
                if property.is_required()
                    && ValueSchema::Number.is_assignable_from(property.schema()) =>
            {
                ScoreStatus::Valid
            }
            Some(property) if schema_contains_dynamic(property.schema()) => ScoreStatus::Dynamic,
            _ => ScoreStatus::Invalid,
        },
        ValueSchema::Union { variants } => variants
            .iter()
            .map(pick_best_score_status)
            .fold(ScoreStatus::Valid, combine_score_status),
        _ => ScoreStatus::Invalid,
    }
}

fn combine_score_status(left: ScoreStatus, right: ScoreStatus) -> ScoreStatus {
    match (left, right) {
        (ScoreStatus::Invalid, _) | (_, ScoreStatus::Invalid) => ScoreStatus::Invalid,
        (ScoreStatus::Dynamic, _) | (_, ScoreStatus::Dynamic) => ScoreStatus::Dynamic,
        _ => ScoreStatus::Valid,
    }
}

fn schema_contains_dynamic(schema: &ValueSchema) -> bool {
    match schema {
        ValueSchema::Dynamic => true,
        ValueSchema::Array { items } => schema_contains_dynamic(items),
        ValueSchema::Object(object) => {
            object
                .property("score")
                .is_some_and(|property| schema_contains_dynamic(property.schema()))
                || object
                    .additional_schema()
                    .is_some_and(schema_contains_dynamic)
        }
        ValueSchema::Union { variants } => variants.iter().any(schema_contains_dynamic),
        _ => false,
    }
}

fn compile_arm(
    arm: &ArmDef,
    location: &str,
    input_schema: &ValueSchema,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<TypedArm> {
    let runner = resolve_runner(
        arm.handler_name(),
        location,
        context,
        diagnostics,
        unresolved,
    )?;
    let expected_input = runner
        .metadata()
        .input_schema
        .as_ref()
        .unwrap_or(&ValueSchema::Dynamic);
    if !expected_input.is_assignable_from(input_schema) {
        diagnostics.push(ValidationDiagnostic::error_with_code(
            ValidationCode::TypeMismatch,
            format!("{location}.input"),
            format!(
                "handler '{}' expects {expected_input}, but receives {input_schema}",
                arm.handler_name()
            ),
        ));
        *unresolved = true;
        return None;
    }
    let args = compile_args(arm.args(), location, context, diagnostics, unresolved)?;
    let output_schema = runner
        .metadata()
        .output_schema
        .clone()
        .unwrap_or(ValueSchema::Dynamic);
    let confidence = runner
        .metadata()
        .confidence
        .unwrap_or(ConfidenceCapability::Optional);

    Some(TypedArm {
        node: arm.clone(),
        runner,
        args,
        output_schema,
        confidence,
    })
}

fn resolve_runner(
    handler_name: &str,
    location: &str,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<std::sync::Arc<dyn crate::step_runner::StepRunner>> {
    let Some(runner) = context.registry.runner(handler_name) else {
        diagnostics.push(diagnostic_for_mode(
            context.options,
            ValidationCode::UnknownHandler,
            location,
            format!("handler '{handler_name}' is not registered"),
        ));
        *unresolved = true;
        return None;
    };
    if !runner.metadata().has_complete_contract() {
        diagnostics.push(diagnostic_for_mode(
            context.options,
            ValidationCode::MissingContract,
            location,
            format!("handler '{handler_name}' has no complete contract"),
        ));
        if context.options.mode() == CompileMode::Strict {
            *unresolved = true;
            return None;
        }
    }
    Some(runner.clone())
}

fn compile_args(
    args: Option<&serde_json::Value>,
    location: &str,
    context: &StepCompileContext<'_>,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> Option<Option<TypedValue>> {
    let expression_diagnostics = RefCell::new(Vec::new());
    let scope = context.reference_scope(&expression_diagnostics, location);
    let resolve = |path: &str| resolve_reference_schema(path, &scope);
    let compiled = match args
        .map(|args| TypedValue::compile_with(args, &resolve))
        .transpose()
    {
        Ok(args) => args,
        Err(error) => {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                validation_code_for_expression(&error),
                format!("{location}.args"),
                error.to_string(),
            ));
            *unresolved = true;
            return None;
        }
    };
    diagnostics.extend(expression_diagnostics.into_inner());
    Some(compiled)
}

fn join_confidence(arms: &[TypedArm]) -> ConfidenceCapability {
    if arms
        .iter()
        .all(|arm| arm.confidence == ConfidenceCapability::Always)
    {
        ConfidenceCapability::Always
    } else if arms
        .iter()
        .all(|arm| arm.confidence == ConfidenceCapability::Never)
    {
        ConfidenceCapability::Never
    } else {
        ConfidenceCapability::Optional
    }
}

fn compile_variables(
    definition: &PipelineDef,
    diagnostics: &mut Vec<ValidationDiagnostic>,
    unresolved: &mut bool,
) -> BTreeMap<String, TypedBinding> {
    let Some(definitions) = &definition.vars else {
        return BTreeMap::new();
    };
    let positions = definitions
        .keys()
        .enumerate()
        .map(|(index, name)| (name.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut variables = BTreeMap::new();

    for (index, (name, value)) in definitions.iter().enumerate() {
        let empty_steps = BTreeMap::new();
        let empty_step_positions = HashMap::new();
        let scope = ReferenceScope {
            input_schema: definition.input_schema.as_ref(),
            variables: &variables,
            variable_positions: &positions,
            current_variable: Some((name.as_str(), index)),
            steps: &empty_steps,
            step_positions: &empty_step_positions,
            current_step: None,
            loop_bindings: &[],
            options: CompileOptions::permissive(),
            diagnostic_sink: None,
        };
        let resolve = |path: &str| resolve_reference_schema(path, &scope);
        match TypedValue::compile_with(value, &resolve) {
            Ok(value) => {
                variables.insert(
                    name.clone(),
                    TypedBinding {
                        id: BindingId(index),
                        value,
                    },
                );
            }
            Err(error) => {
                diagnostics.push(ValidationDiagnostic::error_with_code(
                    validation_code_for_expression(&error),
                    format!("vars.{name}"),
                    error.to_string(),
                ));
                *unresolved = true;
            }
        }
    }
    variables
}

fn resolve_reference_schema(
    path: &str,
    scope: &ReferenceScope<'_>,
) -> Result<ValueSchema, ExprError> {
    if path == "input" {
        return Ok(scope.input_schema.cloned().unwrap_or(ValueSchema::Dynamic));
    }
    if let Some(rest) = path.strip_prefix("input.") {
        return schema_at_path(
            scope.input_schema.unwrap_or(&ValueSchema::Dynamic),
            rest,
            path,
        );
    }
    if let Some(rest) = path.strip_prefix("vars.") {
        let mut parts = rest.splitn(2, '.');
        let name = parts.next().unwrap_or_default();
        if let Some(binding) = scope.variables.get(name) {
            return match parts.next() {
                Some(subpath) => schema_at_path(&binding.value.schema, subpath, path),
                None => Ok(binding.value.schema.clone()),
            };
        }
        if scope
            .current_variable
            .is_some_and(|(current_name, _)| current_name == name)
        {
            return Err(ExprError::InvalidScope(path.to_string()));
        }
        if let (Some((_, current_index)), Some(target_index)) =
            (scope.current_variable, scope.variable_positions.get(name))
            && *target_index >= current_index
        {
            return Err(ExprError::ForwardReference(path.to_string()));
        }
        return Err(ExprError::UnknownPath(path.to_string()));
    }
    if let Some(rest) = path.strip_prefix("steps.") {
        let Some(current_step) = scope.current_step else {
            return Err(ExprError::InvalidScope(path.to_string()));
        };
        let mut parts = rest.splitn(3, '.');
        let name = parts.next().unwrap_or_default();
        let field = parts.next().unwrap_or_default();
        let subpath = parts.next();
        let Some(binding) = scope.steps.get(name) else {
            if scope
                .step_positions
                .get(name)
                .is_some_and(|target| *target >= current_step)
            {
                return Err(ExprError::ForwardReference(path.to_string()));
            }
            return Err(ExprError::UnknownStep(name.to_string()));
        };
        if binding.index >= current_step {
            return Err(ExprError::ForwardReference(path.to_string()));
        }
        return match (field, subpath) {
            ("output", None) => Ok(binding.output.clone()),
            ("output", Some(subpath)) => schema_at_path(&binding.output, subpath, path),
            ("confidence", None) => match binding.confidence {
                ConfidenceCapability::Always => Ok(ValueSchema::Number),
                ConfidenceCapability::Optional
                    if scope.options.mode() == CompileMode::Permissive =>
                {
                    if let Some((sink, location)) = scope.diagnostic_sink {
                        sink.borrow_mut()
                            .push(ValidationDiagnostic::warning_with_code(
                                ValidationCode::DynamicBoundary,
                                location,
                                format!("step '{name}' may not report confidence"),
                            ));
                    }
                    Ok(ValueSchema::Number)
                }
                ConfidenceCapability::Optional | ConfidenceCapability::Never => {
                    Err(ExprError::NoConfidence(name.to_string()))
                }
            },
            _ => Err(ExprError::UnknownPath(path.to_string())),
        };
    }
    if let Some(name) = path.strip_prefix("iter.") {
        for bindings in scope.loop_bindings.iter().rev() {
            if name == "index" {
                return Ok(bindings.index.schema.clone());
            }
            if let Some(item) = &bindings.item
                && item.name == name
            {
                return Ok(item.schema.clone());
            }
        }
        return Err(ExprError::InvalidScope(path.to_string()));
    }
    Err(ExprError::UnknownPath(path.to_string()))
}

fn schema_at_path(
    schema: &ValueSchema,
    path: &str,
    original: &str,
) -> Result<ValueSchema, ExprError> {
    let mut current = schema.clone();
    for segment in path.split('.') {
        current = match current {
            ValueSchema::Dynamic => ValueSchema::Dynamic,
            ValueSchema::Object(object) => object
                .property(segment)
                .map(|property| property.schema().clone())
                .or_else(|| object.additional_schema().cloned())
                .ok_or_else(|| ExprError::UnknownPath(original.to_string()))?,
            ValueSchema::Union { variants } => {
                let variants = variants
                    .iter()
                    .map(|variant| schema_at_path(variant, segment, original))
                    .collect::<Result<Vec<_>, _>>()?;
                ValueSchema::union(variants)
                    .map_err(|_| ExprError::UnknownPath(original.to_string()))?
            }
            _ => return Err(ExprError::UnknownPath(original.to_string())),
        };
    }
    Ok(current)
}

fn validation_code_for_expression(error: &ExprError) -> ValidationCode {
    match error {
        ExprError::ForwardReference(_) => ValidationCode::ForwardReference,
        ExprError::InvalidScope(_) => ValidationCode::InvalidScope,
        ExprError::UnknownPath(_) | ExprError::UnknownStep(_) => ValidationCode::UnknownReference,
        _ => ValidationCode::InvalidExpression,
    }
}

fn diagnostic_for_mode(
    options: CompileOptions,
    code: ValidationCode,
    location: impl Into<String>,
    message: impl Into<String>,
) -> ValidationDiagnostic {
    match options.severity_for(code) {
        DiagnosticSeverity::Error => ValidationDiagnostic::error_with_code(code, location, message),
        DiagnosticSeverity::Warning => {
            ValidationDiagnostic::warning_with_code(code, location, message)
        }
    }
}
