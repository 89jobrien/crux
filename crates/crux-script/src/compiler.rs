//! Typed pipeline compilation options and results.

use std::collections::{BTreeMap, HashMap};

use crate::expr::ExprError;
use crate::ir::{BindingId, TypedBinding, TypedPipeline, TypedStep, TypedValue};
use crate::metadata::ValueSchema;
use crate::registry::HandlerRegistry;
use crate::schema::{PipelineDef, StepDef};
use crate::validator::{DiagnosticSeverity, ValidationCode, ValidationDiagnostic};

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

    for (index, step) in definition.steps.iter().enumerate() {
        let location = format!("steps[{index}]");
        let StepDef::Step(node) = step else {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::InvalidControlFlow,
                location,
                "typed compilation for this combinator is not implemented",
            ));
            unresolved = true;
            continue;
        };

        let handler_name = node.handler.as_deref().unwrap_or(&node.step);
        let Some(runner) = registry.runner(handler_name) else {
            diagnostics.push(diagnostic_for_mode(
                options,
                ValidationCode::UnknownHandler,
                &location,
                format!("handler '{handler_name}' is not registered"),
            ));
            unresolved = true;
            continue;
        };

        if !runner.metadata().has_complete_contract() {
            diagnostics.push(diagnostic_for_mode(
                options,
                ValidationCode::MissingContract,
                &location,
                format!("handler '{handler_name}' has no complete contract"),
            ));
            if options.mode() == CompileMode::Strict {
                unresolved = true;
                continue;
            }
        }

        let resolve = |path: &str| {
            resolve_reference_schema(
                path,
                definition.input_schema.as_ref(),
                &variables,
                &HashMap::new(),
                None,
            )
        };
        let args = match node
            .args
            .as_ref()
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
                unresolved = true;
                continue;
            }
        };

        steps.push(TypedStep {
            node: node.clone(),
            runner,
            args,
        });
    }

    let artifact = (!unresolved).then(|| TypedPipeline::new(definition, variables, steps));
    Compilation::new(artifact, diagnostics)
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
        let resolve = |path: &str| {
            resolve_reference_schema(
                path,
                definition.input_schema.as_ref(),
                &variables,
                &positions,
                Some((name.as_str(), index)),
            )
        };
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
    input_schema: Option<&ValueSchema>,
    variables: &BTreeMap<String, TypedBinding>,
    positions: &HashMap<String, usize>,
    current: Option<(&str, usize)>,
) -> Result<ValueSchema, ExprError> {
    if path == "input" {
        return Ok(input_schema.cloned().unwrap_or(ValueSchema::Dynamic));
    }
    if let Some(rest) = path.strip_prefix("input.") {
        return schema_at_path(input_schema.unwrap_or(&ValueSchema::Dynamic), rest, path);
    }
    if let Some(rest) = path.strip_prefix("vars.") {
        let mut parts = rest.splitn(2, '.');
        let name = parts.next().unwrap_or_default();
        if let Some(binding) = variables.get(name) {
            return match parts.next() {
                Some(subpath) => schema_at_path(&binding.value.schema, subpath, path),
                None => Ok(binding.value.schema.clone()),
            };
        }
        if current.is_some_and(|(current_name, _)| current_name == name) {
            return Err(ExprError::InvalidScope(path.to_string()));
        }
        if let (Some((_, current_index)), Some(target_index)) = (current, positions.get(name))
            && *target_index >= current_index
        {
            return Err(ExprError::ForwardReference(path.to_string()));
        }
        return Err(ExprError::UnknownPath(path.to_string()));
    }
    if path.starts_with("steps.") || path.starts_with("iter.") {
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
