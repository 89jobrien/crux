//! Runtime-only typed pipeline intermediate representation.

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::Arc,
};

use indexmap::IndexMap;
use serde_json::Value;

use crate::expr::{ExprContext, ExprError, ParsedExpression, TemplateSegment, parse_expression};
use crate::metadata::{ConfidenceCapability, ObjectSchema, ValueSchema};
use crate::schema::{
    ArmDef, BudgetDef, ForEachNode, JoinAllNode, OnErrorDef, PipeNode, PipelineDef,
    PipelineDisplayDef, PollNode, RepeatNode, RouteBranch, RouteNode, SpeculateNode, StepNode,
    WhileNode,
};
use crate::step_runner::StepRunner;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum TypedValueKind {
    Literal(Value),
    Expression(ParsedExpression),
    Array(Vec<TypedValue>),
    Object(BTreeMap<String, TypedValue>),
}

#[derive(Debug, Clone)]
pub(crate) struct TypedValue {
    pub(crate) kind: TypedValueKind,
    pub(crate) schema: ValueSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BindingId(pub(crate) usize);

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct TypedBinding {
    pub(crate) id: BindingId,
    pub(crate) value: TypedValue,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct TypedLoopBinding {
    pub(crate) id: BindingId,
    pub(crate) name: String,
    pub(crate) schema: ValueSchema,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct TypedLoopBindings {
    pub(crate) index: TypedLoopBinding,
    pub(crate) item: Option<TypedLoopBinding>,
}

#[derive(Debug, Default)]
struct RuntimeScopeFrame {
    names: HashMap<String, BindingId>,
    values: HashMap<BindingId, Value>,
}

/// Lexical runtime frames keyed by compiler-assigned binding IDs.
#[derive(Debug, Default)]
pub(crate) struct RuntimeScopes {
    frames: Vec<RuntimeScopeFrame>,
}

impl RuntimeScopes {
    pub(crate) fn push_iteration(
        &mut self,
        bindings: &TypedLoopBindings,
        index: usize,
        item: Option<Value>,
    ) {
        let mut frame = RuntimeScopeFrame::default();
        frame
            .names
            .insert(bindings.index.name.clone(), bindings.index.id);
        frame.values.insert(bindings.index.id, Value::from(index));
        if let (Some(binding), Some(value)) = (&bindings.item, item) {
            frame.names.insert(binding.name.clone(), binding.id);
            frame.values.insert(binding.id, value);
        }
        self.frames.push(frame);
    }

    pub(crate) fn pop_iteration(&mut self) {
        let popped = self.frames.pop();
        debug_assert!(popped.is_some(), "iteration scope stack underflow");
    }

    fn resolve(&self, path: &str) -> Option<Result<Value, ExprError>> {
        let rest = path.strip_prefix("iter.")?;
        let (name, tail) = rest
            .split_once('.')
            .map_or((rest, None), |(name, tail)| (name, Some(tail)));
        for frame in self.frames.iter().rev() {
            if let Some(value) = frame
                .names
                .get(name)
                .and_then(|binding_id| frame.values.get(binding_id))
            {
                return Some(match tail {
                    Some(tail) => value_at_path(value, tail)
                        .ok_or_else(|| ExprError::UnknownPath(path.to_string())),
                    None => Ok(value.clone()),
                });
            }
        }
        Some(Err(ExprError::UnknownPath(path.to_string())))
    }
}

impl TypedValue {
    pub(crate) fn compile_with<F>(value: &Value, resolve: &F) -> Result<Self, ExprError>
    where
        F: Fn(&str) -> Result<ValueSchema, ExprError>,
    {
        match value {
            Value::Null => Ok(Self::literal(value, ValueSchema::Null)),
            Value::Bool(_) => Ok(Self::literal(value, ValueSchema::Boolean)),
            Value::Number(number) if number.is_i64() || number.is_u64() => {
                Ok(Self::literal(value, ValueSchema::Integer))
            }
            Value::Number(_) => Ok(Self::literal(value, ValueSchema::Number)),
            Value::String(expression) => {
                let parsed = parse_expression(expression)?;
                let schema = match &parsed {
                    ParsedExpression::ExactPath(path) => resolve(path)?,
                    ParsedExpression::Literal(_) => ValueSchema::String,
                    ParsedExpression::Interpolated(segments) => {
                        for segment in segments {
                            if let crate::expr::TemplateSegment::Path(path) = segment {
                                resolve(path)?;
                            }
                        }
                        ValueSchema::String
                    }
                };
                Ok(Self {
                    kind: TypedValueKind::Expression(parsed),
                    schema,
                })
            }
            Value::Array(values) => {
                let values = values
                    .iter()
                    .map(|value| Self::compile_with(value, resolve))
                    .collect::<Result<Vec<_>, _>>()?;
                let schema = if values.is_empty() {
                    ValueSchema::array(ValueSchema::Dynamic)
                } else {
                    let item_schema =
                        ValueSchema::union(values.iter().map(|value| value.schema.clone()))
                            .unwrap_or(ValueSchema::Dynamic);
                    ValueSchema::array(item_schema)
                };
                Ok(Self {
                    kind: TypedValueKind::Array(values),
                    schema,
                })
            }
            Value::Object(values) => {
                let values = values
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), Self::compile_with(value, resolve)?)))
                    .collect::<Result<BTreeMap<_, _>, ExprError>>()?;
                let schema = values
                    .iter()
                    .fold(ObjectSchema::new(), |schema, (name, value)| {
                        schema.required(name, value.schema.clone())
                    });
                Ok(Self {
                    kind: TypedValueKind::Object(values),
                    schema: ValueSchema::object(schema),
                })
            }
        }
    }

    fn literal(value: &Value, schema: ValueSchema) -> Self {
        Self {
            kind: TypedValueKind::Literal(value.clone()),
            schema,
        }
    }

    fn object_property_schema(&self, property: &str) -> Option<&ValueSchema> {
        let TypedValueKind::Object(values) = &self.kind else {
            return None;
        };
        values.get(property).map(|value| &value.schema)
    }

    pub(crate) fn evaluate(&self, context: &ExprContext) -> Result<Value, ExprError> {
        self.evaluate_scoped(context, &RuntimeScopes::default())
    }

    pub(crate) fn evaluate_scoped(
        &self,
        context: &ExprContext,
        scopes: &RuntimeScopes,
    ) -> Result<Value, ExprError> {
        match &self.kind {
            TypedValueKind::Literal(value) => Ok(value.clone()),
            TypedValueKind::Expression(expression) => match expression {
                ParsedExpression::Literal(value) => Ok(Value::String(value.clone())),
                ParsedExpression::ExactPath(path) => resolve_path(context, scopes, path),
                ParsedExpression::Interpolated(segments) => {
                    let mut value = String::new();
                    for segment in segments {
                        match segment {
                            TemplateSegment::Text(text) => value.push_str(text),
                            TemplateSegment::Path(path) => {
                                match resolve_path(context, scopes, path)? {
                                    Value::String(text) => value.push_str(&text),
                                    resolved => value.push_str(&resolved.to_string()),
                                }
                            }
                        }
                    }
                    Ok(Value::String(value))
                }
            },
            TypedValueKind::Array(values) => values
                .iter()
                .map(|value| value.evaluate_scoped(context, scopes))
                .collect(),
            TypedValueKind::Object(values) => values
                .iter()
                .map(|(name, value)| Ok((name.clone(), value.evaluate_scoped(context, scopes)?)))
                .collect(),
        }
    }

    fn reads_handler_confidence(&self) -> bool {
        match &self.kind {
            TypedValueKind::Literal(_) => false,
            TypedValueKind::Expression(ParsedExpression::Literal(_)) => false,
            TypedValueKind::Expression(ParsedExpression::ExactPath(path)) => {
                is_handler_confidence_path(path)
            }
            TypedValueKind::Expression(ParsedExpression::Interpolated(segments)) => {
                segments.iter().any(|segment| {
                    matches!(segment, TemplateSegment::Path(path) if is_handler_confidence_path(path))
                })
            }
            TypedValueKind::Array(values) => {
                values.iter().any(TypedValue::reads_handler_confidence)
            }
            TypedValueKind::Object(values) => {
                values.values().any(TypedValue::reads_handler_confidence)
            }
        }
    }
}

fn is_handler_confidence_path(path: &str) -> bool {
    path.strip_prefix("steps.")
        .and_then(|path| path.split_once('.'))
        .is_some_and(|(_, field)| field == "confidence")
}

fn resolve_path(
    context: &ExprContext,
    scopes: &RuntimeScopes,
    path: &str,
) -> Result<Value, ExprError> {
    scopes
        .resolve(path)
        .unwrap_or_else(|| context.eval(&format!("{{{{ {path} }}}}")))
}

fn value_at_path(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current.clone())
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct TypedHandlerStep {
    pub(crate) node: StepNode,
    pub(crate) runner: Arc<dyn StepRunner>,
    pub(crate) args: Option<TypedValue>,
    pub(crate) recovery: Option<TypedRecoveryStep>,
}

impl fmt::Debug for TypedHandlerStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedHandlerStep")
            .field("name", &self.node.step)
            .field("handler", &self.runner.metadata().name)
            .field("args", &self.args)
            .field("recovery", &self.recovery)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct TypedRecoveryStep {
    pub(crate) node: OnErrorDef,
    pub(crate) runner: Arc<dyn StepRunner>,
    pub(crate) args: Option<TypedValue>,
}

impl fmt::Debug for TypedRecoveryStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedRecoveryStep")
            .field("handler", &self.runner.metadata().name)
            .field("args", &self.args)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct TypedArm {
    pub(crate) node: ArmDef,
    pub(crate) runner: Arc<dyn StepRunner>,
    pub(crate) args: Option<TypedValue>,
    pub(crate) output_schema: ValueSchema,
    pub(crate) confidence: ConfidenceCapability,
}

impl fmt::Debug for TypedArm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedArm")
            .field("label", &self.node.label())
            .field("handler", &self.runner.metadata().name)
            .field("args", &self.args)
            .field("output_schema", &self.output_schema)
            .field("confidence", &self.confidence)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct TypedRouteBranch {
    pub(crate) node: RouteBranch,
    pub(crate) runner: Arc<dyn StepRunner>,
    pub(crate) args: Option<TypedValue>,
    pub(crate) output_schema: ValueSchema,
    pub(crate) confidence: ConfidenceCapability,
}

impl fmt::Debug for TypedRouteBranch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedRouteBranch")
            .field("label", &self.node.label)
            .field("handler", &self.runner.metadata().name)
            .field("args", &self.args)
            .field("output_schema", &self.output_schema)
            .field("confidence", &self.confidence)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum TypedStepKind {
    Handler(Box<TypedHandlerStep>),
    Pipe {
        node: PipeNode,
        stages: Vec<TypedArm>,
    },
    JoinAll {
        node: JoinAllNode,
        arms: Vec<TypedArm>,
    },
    RouteOnConfidence {
        node: RouteNode,
        value: TypedValue,
        branches: Vec<TypedRouteBranch>,
    },
    Speculate {
        node: SpeculateNode,
        arms: Vec<TypedArm>,
    },
    Poll {
        node: PollNode,
        bindings: TypedLoopBindings,
        body: Vec<TypedStep>,
        until: TypedValue,
    },
    ForEach {
        node: ForEachNode,
        items: TypedValue,
        bindings: TypedLoopBindings,
        body: Vec<TypedStep>,
        break_if: Option<TypedValue>,
    },
    While {
        node: WhileNode,
        bindings: TypedLoopBindings,
        condition: TypedValue,
        body: Vec<TypedStep>,
        break_if: Option<TypedValue>,
    },
    Repeat {
        node: RepeatNode,
        bindings: TypedLoopBindings,
        body: Vec<TypedStep>,
        break_if: Option<TypedValue>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct TypedStep {
    pub(crate) name: String,
    pub(crate) kind: TypedStepKind,
    pub(crate) output_schema: ValueSchema,
    pub(crate) confidence: ConfidenceCapability,
}

impl TypedStep {
    fn reads_handler_confidence(&self) -> bool {
        let arm_reads_confidence = |arm: &TypedArm| {
            arm.args
                .as_ref()
                .is_some_and(TypedValue::reads_handler_confidence)
        };
        let route_reads_confidence = |branch: &TypedRouteBranch| {
            branch
                .args
                .as_ref()
                .is_some_and(TypedValue::reads_handler_confidence)
        };
        let body_reads_confidence =
            |body: &[TypedStep]| body.iter().any(TypedStep::reads_handler_confidence);

        match &self.kind {
            TypedStepKind::Handler(handler) => {
                handler
                    .args
                    .as_ref()
                    .is_some_and(TypedValue::reads_handler_confidence)
                    || handler.recovery.as_ref().is_some_and(|recovery| {
                        recovery
                            .args
                            .as_ref()
                            .is_some_and(TypedValue::reads_handler_confidence)
                    })
            }
            TypedStepKind::Pipe { stages, .. }
            | TypedStepKind::JoinAll { arms: stages, .. }
            | TypedStepKind::Speculate { arms: stages, .. } => {
                stages.iter().any(arm_reads_confidence)
            }
            TypedStepKind::RouteOnConfidence {
                value, branches, ..
            } => value.reads_handler_confidence() || branches.iter().any(route_reads_confidence),
            TypedStepKind::Poll { body, until, .. } => {
                body_reads_confidence(body) || until.reads_handler_confidence()
            }
            TypedStepKind::ForEach {
                items,
                body,
                break_if,
                ..
            } => {
                items.reads_handler_confidence()
                    || body_reads_confidence(body)
                    || break_if
                        .as_ref()
                        .is_some_and(TypedValue::reads_handler_confidence)
            }
            TypedStepKind::While {
                condition,
                body,
                break_if,
                ..
            } => {
                condition.reads_handler_confidence()
                    || body_reads_confidence(body)
                    || break_if
                        .as_ref()
                        .is_some_and(TypedValue::reads_handler_confidence)
            }
            TypedStepKind::Repeat { body, break_if, .. } => {
                body_reads_confidence(body)
                    || break_if
                        .as_ref()
                        .is_some_and(TypedValue::reads_handler_confidence)
            }
        }
    }
}

pub(crate) fn failed_allowed_output_schema() -> ValueSchema {
    ValueSchema::object(
        ObjectSchema::new()
            .required("status", ValueSchema::String)
            .required("error", ValueSchema::String),
    )
}

/// Pipeline definition whose simple handler names have been resolved to executors.
#[derive(Clone)]
pub struct TypedPipeline {
    pub(crate) name: String,
    pub(crate) input_schema: Option<ValueSchema>,
    pub(crate) variables: BTreeMap<String, TypedBinding>,
    pub(crate) steps: Vec<TypedStep>,
    pub(crate) budget: Option<BudgetDef>,
    pub(crate) display: Option<PipelineDisplayDef>,
    confidence_dependent: bool,
}

impl TypedPipeline {
    pub(crate) fn new(
        definition: &PipelineDef,
        variables: BTreeMap<String, TypedBinding>,
        steps: Vec<TypedStep>,
    ) -> Self {
        let confidence_dependent = variables
            .values()
            .any(|binding| binding.value.reads_handler_confidence())
            || steps.iter().any(TypedStep::reads_handler_confidence);
        Self {
            name: definition.pipeline.clone(),
            input_schema: definition.input_schema.clone(),
            variables,
            steps,
            budget: definition.budget.clone(),
            display: definition.display.clone(),
            confidence_dependent,
        }
    }

    /// Return the stable pipeline name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the declared pipeline input schema.
    pub fn input_schema(&self) -> Option<&ValueSchema> {
        self.input_schema.as_ref()
    }

    /// Return the inferred schema for one pipeline variable.
    pub fn variable_schema(&self, name: &str) -> Option<&ValueSchema> {
        self.variables
            .get(name)
            .map(|binding| &binding.value.schema)
    }

    /// Return the number of compiled top-level steps.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    pub(crate) fn supports_simple_execution(&self) -> bool {
        self.steps
            .iter()
            .all(|step| matches!(step.kind, TypedStepKind::Handler(_)))
    }

    pub(crate) fn is_confidence_dependent(&self) -> bool {
        self.confidence_dependent
    }

    /// Return the inferred schema for one top-level step argument.
    pub fn step_argument_schema(&self, step: &str, argument: &str) -> Option<&ValueSchema> {
        self.steps
            .iter()
            .find(|typed| typed.name == step)
            .and_then(|typed| match &typed.kind {
                TypedStepKind::Handler(handler) => handler.args.as_ref(),
                TypedStepKind::Pipe { .. }
                | TypedStepKind::JoinAll { .. }
                | TypedStepKind::RouteOnConfidence { .. }
                | TypedStepKind::Speculate { .. }
                | TypedStepKind::Poll { .. }
                | TypedStepKind::ForEach { .. }
                | TypedStepKind::While { .. }
                | TypedStepKind::Repeat { .. } => None,
            })
            .and_then(|args| args.object_property_schema(argument))
    }

    /// Return an inferred argument schema from a direct child step of a loop body.
    pub fn loop_body_step_argument_schema(
        &self,
        loop_step: &str,
        body_step: &str,
        argument: &str,
    ) -> Option<&ValueSchema> {
        self.steps
            .iter()
            .find(|typed| typed.name == loop_step)
            .and_then(|typed| match &typed.kind {
                TypedStepKind::Poll { body, .. }
                | TypedStepKind::ForEach { body, .. }
                | TypedStepKind::While { body, .. }
                | TypedStepKind::Repeat { body, .. } => Some(body),
                _ => None,
            })
            .and_then(|body| body.iter().find(|typed| typed.name == body_step))
            .and_then(|typed| match &typed.kind {
                TypedStepKind::Handler(handler) => handler.args.as_ref(),
                _ => None,
            })
            .and_then(|args| args.object_property_schema(argument))
    }

    /// Return the inferred successful output schema for one top-level step.
    pub fn step_output_schema(&self, step: &str) -> Option<&ValueSchema> {
        self.steps
            .iter()
            .find(|typed| typed.name == step)
            .map(|typed| &typed.output_schema)
    }

    /// Return the inferred confidence capability for one top-level step.
    pub fn step_confidence(&self, step: &str) -> Option<ConfidenceCapability> {
        self.steps
            .iter()
            .find(|typed| typed.name == step)
            .map(|typed| typed.confidence)
    }
}

impl fmt::Debug for TypedPipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedPipeline")
            .field("name", &self.name)
            .field("input_schema", &self.input_schema)
            .field("variables", &self.variables)
            .field("steps", &self.steps)
            .field("budget", &self.budget)
            .field("display", &self.display)
            .field("confidence_dependent", &self.confidence_dependent)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TypedTarget {
    pub(crate) dependencies: Vec<String>,
    pub(crate) pipeline: TypedPipeline,
}

/// Fully compiled multi-target Cruxfile.
#[derive(Clone)]
pub struct TypedCruxfile {
    pub(crate) project: String,
    pub(crate) default_target: String,
    pub(crate) targets: IndexMap<String, TypedTarget>,
    pub(crate) target_order: Vec<String>,
}

impl TypedCruxfile {
    pub(crate) fn new(
        project: String,
        default_target: String,
        targets: IndexMap<String, TypedTarget>,
        target_order: Vec<String>,
    ) -> Self {
        Self {
            project,
            default_target,
            targets,
            target_order,
        }
    }

    /// Return the Cruxfile project name.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Return the default target name.
    pub fn default_target(&self) -> &str {
        &self.default_target
    }

    /// Return whether the compiled Cruxfile contains a target.
    pub fn contains_target(&self, name: &str) -> bool {
        self.targets.contains_key(name)
    }

    /// Return every compiled target in stable dependency order.
    pub fn target_order(&self) -> &[String] {
        &self.target_order
    }

    /// Return one target's dependencies in declaration order.
    pub fn target_dependencies(&self, name: &str) -> Option<&[String]> {
        self.targets
            .get(name)
            .map(|target| target.dependencies.as_slice())
    }

    /// Return one target's compiled pipeline.
    pub fn target(&self, name: &str) -> Option<&TypedPipeline> {
        self.targets.get(name).map(|target| &target.pipeline)
    }
}

impl fmt::Debug for TypedCruxfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedCruxfile")
            .field("project", &self.project)
            .field("default_target", &self.default_target)
            .field("targets", &self.targets)
            .field("target_order", &self.target_order)
            .finish()
    }
}
