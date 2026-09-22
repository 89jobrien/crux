//! Handler metadata and lightweight static argument schemas.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Recursive JSON value schema used by pipeline contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", content = "definition", rename_all = "snake_case")]
pub enum ValueSchema {
    /// Explicitly dynamic value accepted without structural validation.
    Dynamic,
    /// JSON null.
    Null,
    /// JSON boolean.
    Boolean,
    /// Integral JSON number.
    Integer,
    /// Any JSON number, including integers.
    Number,
    /// JSON string.
    String,
    /// JSON array with a uniform item schema.
    Array { items: Box<ValueSchema> },
    /// JSON object with named and optional additional properties.
    Object(ObjectSchema),
    /// Value accepted by any one of the member schemas.
    Union { variants: Vec<ValueSchema> },
}

impl ValueSchema {
    /// Create an array value schema.
    pub fn array(items: ValueSchema) -> Self {
        Self::Array {
            items: Box::new(items),
        }
    }

    /// Create an object value schema.
    pub fn object(schema: ObjectSchema) -> Self {
        Self::Object(schema)
    }

    /// Create a flattened, deduplicated union schema.
    pub fn union(
        variants: impl IntoIterator<Item = ValueSchema>,
    ) -> Result<Self, SchemaBuildError> {
        let mut normalized = Vec::new();
        for variant in variants {
            Self::push_union_variant(&mut normalized, variant);
        }

        match normalized.len() {
            0 => Err(SchemaBuildError::EmptyUnion),
            1 => Ok(normalized.remove(0)),
            _ => Ok(Self::Union {
                variants: normalized,
            }),
        }
    }

    fn push_union_variant(normalized: &mut Vec<Self>, variant: Self) {
        match variant {
            Self::Union { variants } => {
                for nested in variants {
                    Self::push_union_variant(normalized, nested);
                }
            }
            variant if !normalized.contains(&variant) => normalized.push(variant),
            _ => {}
        }
    }

    /// Validate recursive schema invariants after deserialization.
    pub fn validate_definition(&self) -> Result<(), SchemaBuildError> {
        match self {
            Self::Array { items } => items.validate_definition(),
            Self::Object(schema) => schema.validate_definition(),
            Self::Union { variants } => {
                if variants.is_empty() {
                    return Err(SchemaBuildError::EmptyUnion);
                }
                variants.iter().try_for_each(Self::validate_definition)
            }
            _ => Ok(()),
        }
    }

    /// Return whether a value described by `source` can flow into this schema.
    pub fn is_assignable_from(&self, source: &Self) -> bool {
        match (self, source) {
            (Self::Dynamic, _) => true,
            (_, Self::Union { variants }) => variants
                .iter()
                .all(|source| self.is_assignable_from(source)),
            (Self::Union { variants }, _) => variants
                .iter()
                .any(|target| target.is_assignable_from(source)),
            (Self::Number, Self::Integer) => true,
            (Self::Array { items: target }, Self::Array { items: source }) => {
                target.is_assignable_from(source)
            }
            (Self::Object(target), Self::Object(source)) => target.is_assignable_from(source),
            _ => self == source,
        }
    }

    /// Validate one JSON value against this schema.
    pub fn validate(&self, value: &Value) -> Result<(), SchemaViolation> {
        self.validate_at(value, "$")
    }

    fn validate_at(&self, value: &Value, path: &str) -> Result<(), SchemaViolation> {
        let matches = match self {
            Self::Dynamic => true,
            Self::Null => value.is_null(),
            Self::Boolean => value.is_boolean(),
            Self::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
            Self::Number => value.is_number(),
            Self::String => value.is_string(),
            Self::Array { items } => {
                if let Some(array) = value.as_array() {
                    for (index, value) in array.iter().enumerate() {
                        items.validate_at(value, &format!("{path}[{index}]"))?;
                    }
                    true
                } else {
                    false
                }
            }
            Self::Object(schema) => {
                if let Some(object) = value.as_object() {
                    schema.validate_object(object, path)?;
                    true
                } else {
                    false
                }
            }
            Self::Union { variants } => variants
                .iter()
                .any(|variant| variant.validate_at(value, path).is_ok()),
        };

        if matches {
            Ok(())
        } else {
            Err(SchemaViolation {
                path: path.to_string(),
                expected: self.clone(),
                kind: SchemaViolationKind::TypeMismatch {
                    actual: ValueKind::from_value(value),
                },
            })
        }
    }
}

impl fmt::Display for ValueSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Dynamic => "dynamic",
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array { .. } => "array",
            Self::Object(_) => "object",
            Self::Union { .. } => "union",
        };
        f.write_str(name)
    }
}

/// Schema for a JSON object and its named properties.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectSchema {
    properties: BTreeMap<String, SchemaProperty>,
    additional: Option<Box<ValueSchema>>,
}

impl ObjectSchema {
    /// Create a closed object schema with no declared properties.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a required property.
    pub fn required(mut self, name: impl Into<String>, schema: ValueSchema) -> Self {
        self.properties
            .insert(name.into(), SchemaProperty::new(schema, true));
        self
    }

    /// Add an optional property.
    pub fn optional(mut self, name: impl Into<String>, schema: ValueSchema) -> Self {
        self.properties
            .insert(name.into(), SchemaProperty::new(schema, false));
        self
    }

    /// Permit additional properties matching `schema`.
    pub fn additional(mut self, schema: ValueSchema) -> Self {
        self.additional = Some(Box::new(schema));
        self
    }

    /// Return one declared property.
    pub fn property(&self, name: &str) -> Option<&SchemaProperty> {
        self.properties.get(name)
    }

    /// Return the schema for additional properties, if they are allowed.
    pub fn additional_schema(&self) -> Option<&ValueSchema> {
        self.additional.as_deref()
    }

    fn is_assignable_from(&self, source: &Self) -> bool {
        let declared_properties_match =
            self.properties
                .iter()
                .all(|(name, target)| match source.properties.get(name) {
                    Some(source) => {
                        (!target.required || source.required)
                            && target.schema.is_assignable_from(&source.schema)
                    }
                    None => {
                        !target.required
                            && source
                                .additional
                                .as_deref()
                                .is_none_or(|source| target.schema.is_assignable_from(source))
                    }
                });
        if !declared_properties_match {
            return false;
        }

        let source_extras_match = source
            .properties
            .iter()
            .filter(|(name, _)| !self.properties.contains_key(*name))
            .all(|(_, property)| {
                self.additional
                    .as_deref()
                    .is_some_and(|target| target.is_assignable_from(&property.schema))
            });
        if !source_extras_match {
            return false;
        }

        match (self.additional.as_deref(), source.additional.as_deref()) {
            (None, Some(_)) => false,
            (Some(target), Some(source)) => target.is_assignable_from(source),
            _ => true,
        }
    }

    fn validate_object(
        &self,
        object: &serde_json::Map<String, Value>,
        path: &str,
    ) -> Result<(), SchemaViolation> {
        for (name, property) in &self.properties {
            let property_path = format!("{path}.{name}");
            match object.get(name) {
                Some(value) => property.schema.validate_at(value, &property_path)?,
                None if property.required => {
                    return Err(SchemaViolation {
                        path: property_path,
                        expected: property.schema.clone(),
                        kind: SchemaViolationKind::MissingRequiredProperty,
                    });
                }
                None => {}
            }
        }

        for (name, value) in object {
            if self.properties.contains_key(name) {
                continue;
            }
            let property_path = format!("{path}.{name}");
            match self.additional.as_deref() {
                Some(schema) => schema.validate_at(value, &property_path)?,
                None => {
                    return Err(SchemaViolation {
                        path: property_path,
                        expected: ValueSchema::Object(self.clone()),
                        kind: SchemaViolationKind::AdditionalPropertyNotAllowed,
                    });
                }
            }
        }

        Ok(())
    }

    fn validate_definition(&self) -> Result<(), SchemaBuildError> {
        self.properties
            .values()
            .map(SchemaProperty::schema)
            .chain(self.additional_schema())
            .try_for_each(ValueSchema::validate_definition)
    }
}

/// One named property in an object schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaProperty {
    schema: ValueSchema,
    required: bool,
}

impl SchemaProperty {
    fn new(schema: ValueSchema, required: bool) -> Self {
        Self { schema, required }
    }

    /// Return the property's value schema.
    pub fn schema(&self) -> &ValueSchema {
        &self.schema
    }

    /// Return whether the property must be present.
    pub fn is_required(&self) -> bool {
        self.required
    }
}

/// Concrete JSON kind observed during runtime contract validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    /// JSON null.
    Null,
    /// JSON boolean.
    Boolean,
    /// Integral JSON number.
    Integer,
    /// Non-integral JSON number.
    Number,
    /// JSON string.
    String,
    /// JSON array.
    Array,
    /// JSON object.
    Object,
}

impl ValueKind {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Boolean,
            Value::Number(number) if number.is_i64() || number.is_u64() => Self::Integer,
            Value::Number(_) => Self::Number,
            Value::String(_) => Self::String,
            Value::Array(_) => Self::Array,
            Value::Object(_) => Self::Object,
        }
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        };
        f.write_str(name)
    }
}

/// Specific reason a runtime value failed schema validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaViolationKind {
    /// The runtime value has an incompatible JSON kind.
    #[error("got {actual}")]
    TypeMismatch { actual: ValueKind },
    /// A required object property is absent.
    #[error("required property is missing")]
    MissingRequiredProperty,
    /// A closed object contains an undeclared property.
    #[error("additional property is not allowed")]
    AdditionalPropertyNotAllowed,
}

/// Runtime mismatch between a JSON value and its declared schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("schema mismatch at {path}: expected {expected}, {kind}")]
pub struct SchemaViolation {
    /// JSON path at which validation failed.
    pub path: String,
    /// Schema expected at the failing path.
    pub expected: ValueSchema,
    /// Reason validation failed.
    pub kind: SchemaViolationKind,
}

/// Invalid recursive schema definition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaBuildError {
    /// A union must contain at least one possible schema.
    #[error("union schema must contain at least one variant")]
    EmptyUnion,
}

/// Static JSON type accepted by a handler argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgType {
    Any,
    String,
    Number,
    Integer,
    Boolean,
    Object,
    Array,
}

impl ArgType {
    pub fn matches(self, value: &Value) -> bool {
        match self {
            ArgType::Any => true,
            ArgType::String => value.is_string(),
            ArgType::Number => value.is_number(),
            ArgType::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
            ArgType::Boolean => value.is_boolean(),
            ArgType::Object => value.is_object(),
            ArgType::Array => value.is_array(),
        }
    }
}

impl From<ArgType> for ValueSchema {
    fn from(arg_type: ArgType) -> Self {
        match arg_type {
            ArgType::Any => Self::Dynamic,
            ArgType::String => Self::String,
            ArgType::Number => Self::Number,
            ArgType::Integer => Self::Integer,
            ArgType::Boolean => Self::Boolean,
            ArgType::Object => Self::object(ObjectSchema::new().additional(Self::Dynamic)),
            ArgType::Array => Self::array(Self::Dynamic),
        }
    }
}

/// One static argument accepted by a handler under the pipeline `args` object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSpec {
    pub name: String,
    pub schema: ValueSchema,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ArgSpec {
    pub fn required(name: impl Into<String>, schema: impl Into<ValueSchema>) -> Self {
        Self {
            name: name.into(),
            schema: schema.into(),
            required: true,
            description: None,
        }
    }

    pub fn optional(name: impl Into<String>, schema: impl Into<ValueSchema>) -> Self {
        Self {
            name: name.into(),
            schema: schema.into(),
            required: false,
            description: None,
        }
    }

    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Static `args` schema for a handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSchema {
    #[serde(default)]
    pub args: Vec<ArgSpec>,
    #[serde(default = "default_allow_extra_args")]
    pub allow_extra: bool,
}

impl ArgSchema {
    pub fn new() -> Self {
        Self {
            args: Vec::new(),
            allow_extra: true,
        }
    }

    pub fn strict() -> Self {
        Self {
            args: Vec::new(),
            allow_extra: false,
        }
    }

    pub fn required(mut self, name: impl Into<String>, schema: impl Into<ValueSchema>) -> Self {
        self.args.push(ArgSpec::required(name, schema));
        self
    }

    pub fn optional(mut self, name: impl Into<String>, schema: impl Into<ValueSchema>) -> Self {
        self.args.push(ArgSpec::optional(name, schema));
        self
    }

    pub fn allow_extra(mut self, allow_extra: bool) -> Self {
        self.allow_extra = allow_extra;
        self
    }

    pub fn get(&self, name: &str) -> Option<&ArgSpec> {
        self.args.iter().find(|spec| spec.name == name)
    }

    pub fn has_required_args(&self) -> bool {
        self.args.iter().any(|spec| spec.required)
    }
}

impl Default for ArgSchema {
    fn default() -> Self {
        Self::new()
    }
}

fn default_allow_extra_args() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    None,
    ReadFs,
    WriteFs,
    Shell,
    Network,
    Git,
    Docker,
    Llm,
    Database,
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadFs,
    WriteFs,
    Shell,
    Network,
    Git,
    Docker,
    Llm,
    Database,
    Process,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("handler '{handler}' requires unapproved capabilities: {missing:?}")]
pub struct CapabilityViolation {
    pub handler: String,
    pub missing: Vec<Capability>,
}

/// Whether a handler reports a confidence score with its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceCapability {
    /// The handler never reports confidence.
    Never,
    /// The handler may report confidence depending on the result.
    Optional,
    /// Every successful result reports confidence.
    Always,
}

/// Static input and output contract for a delegated pipeline agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMetadata {
    /// Name used by delegate nodes.
    pub name: String,
    /// Accepted input schema, or `None` for a legacy dynamic agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<ValueSchema>,
    /// Successful output schema, or `None` for a legacy dynamic agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<ValueSchema>,
}

impl AgentMetadata {
    /// Create metadata for a named agent without typed schemas.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            input_schema: None,
            output_schema: None,
        }
    }

    /// Set the accepted input schema.
    pub fn input_schema(mut self, schema: ValueSchema) -> Self {
        self.input_schema = Some(schema);
        self
    }

    /// Set the successful output schema.
    pub fn output_schema(mut self, schema: ValueSchema) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Return whether both typed contract fields are declared.
    pub fn has_complete_contract(&self) -> bool {
        self.input_schema.is_some() && self.output_schema.is_some()
    }
}

/// Introspection metadata for a registered handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandlerMetadata {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub args: ArgSchema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<ValueSchema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<ValueSchema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceCapability>,
    pub risk: RiskLevel,
    #[serde(default)]
    pub side_effects: Vec<SideEffect>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default = "default_deterministic")]
    pub deterministic: bool,
}

impl HandlerMetadata {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            args: ArgSchema::new(),
            input_schema: None,
            output_schema: None,
            confidence: None,
            risk: RiskLevel::Low,
            side_effects: vec![SideEffect::None],
            capabilities: Vec::new(),
            deterministic: true,
        }
    }

    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn args(mut self, args: ArgSchema) -> Self {
        self.args = args;
        self
    }

    /// Set the upstream pipeline input schema.
    pub fn input_schema(mut self, schema: ValueSchema) -> Self {
        self.input_schema = Some(schema);
        self
    }

    /// Set the successful output schema.
    pub fn output_schema(mut self, schema: ValueSchema) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Set the confidence-reporting contract.
    pub fn confidence(mut self, capability: ConfidenceCapability) -> Self {
        self.confidence = Some(capability);
        self
    }

    /// Return whether every typed contract field is declared.
    pub fn has_complete_contract(&self) -> bool {
        self.input_schema.is_some() && self.output_schema.is_some() && self.confidence.is_some()
    }

    pub fn risk(mut self, risk: RiskLevel) -> Self {
        self.risk = risk;
        self
    }

    pub fn side_effects(mut self, side_effects: impl Into<Vec<SideEffect>>) -> Self {
        self.side_effects = side_effects.into();
        self
    }

    pub fn capabilities(mut self, capabilities: impl Into<Vec<Capability>>) -> Self {
        self.capabilities = capabilities.into();
        self
    }

    pub fn authorize_capabilities(
        &self,
        approved: &[Capability],
    ) -> Result<(), CapabilityViolation> {
        let missing: Vec<_> = self
            .capabilities
            .iter()
            .copied()
            .filter(|capability| !approved.contains(capability))
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(CapabilityViolation {
                handler: self.name.clone(),
                missing,
            })
        }
    }

    pub fn deterministic(mut self, deterministic: bool) -> Self {
        self.deterministic = deterministic;
        self
    }
}

fn default_deterministic() -> bool {
    true
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn handler_capabilities_require_explicit_approval() {
        let handler = HandlerMetadata::new("shell").capabilities(vec![Capability::Shell]);

        assert!(
            handler
                .authorize_capabilities(&[Capability::ReadFs])
                .is_err()
        );
        assert!(handler.authorize_capabilities(&[Capability::Shell]).is_ok());
    }
}
