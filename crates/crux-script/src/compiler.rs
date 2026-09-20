//! Typed pipeline compilation options and results.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use crate::expr::ExprError;
use crate::ir::{
    BindingId, TypedArm, TypedBinding, TypedHandlerStep, TypedPipeline, TypedStep, TypedStepKind,
    TypedValue,
};
use crate::metadata::{ConfidenceCapability, ValueSchema};
use crate::registry::HandlerRegistry;
use crate::schema::{ArmDef, JoinAllNode, PipeNode, PipelineDef, StepDef, StepNode};
use crate::validator::{DiagnosticSeverity, ValidationCode, ValidationDiagnostic};

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
            _ => None,
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
        _ => None,
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
    if path.starts_with("iter.") {
        return Ok(ValueSchema::Dynamic);
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
