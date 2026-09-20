//! Runtime-only typed pipeline intermediate representation.

use std::{collections::BTreeMap, fmt, sync::Arc};

use serde_json::Value;

use crate::expr::{ExprError, ParsedExpression, parse_expression};
use crate::metadata::{ObjectSchema, ValueSchema};
use crate::schema::{BudgetDef, PipelineDef, PipelineDisplayDef, StepNode};
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
}

#[derive(Clone)]
pub(crate) struct TypedStep {
    pub(crate) node: StepNode,
    pub(crate) runner: Arc<dyn StepRunner>,
    pub(crate) args: Option<TypedValue>,
}

impl fmt::Debug for TypedStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedStep")
            .field("name", &self.node.step)
            .field("handler", &self.runner.metadata().name)
            .field("args", &self.args)
            .finish_non_exhaustive()
    }
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
}

impl TypedPipeline {
    pub(crate) fn new(
        definition: &PipelineDef,
        variables: BTreeMap<String, TypedBinding>,
        steps: Vec<TypedStep>,
    ) -> Self {
        Self {
            name: definition.pipeline.clone(),
            input_schema: definition.input_schema.clone(),
            variables,
            steps,
            budget: definition.budget.clone(),
            display: definition.display.clone(),
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

    /// Return the inferred schema for one top-level step argument.
    pub fn step_argument_schema(&self, step: &str, argument: &str) -> Option<&ValueSchema> {
        self.steps
            .iter()
            .find(|typed| typed.node.step == step)
            .and_then(|typed| typed.args.as_ref())
            .and_then(|args| args.object_property_schema(argument))
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
            .finish()
    }
}
