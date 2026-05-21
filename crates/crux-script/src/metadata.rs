//! Handler metadata and lightweight static argument schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// One static argument accepted by a handler under the pipeline `args` object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSpec {
    pub name: String,
    pub arg_type: ArgType,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ArgSpec {
    pub fn required(name: impl Into<String>, arg_type: ArgType) -> Self {
        Self {
            name: name.into(),
            arg_type,
            required: true,
            description: None,
        }
    }

    pub fn optional(name: impl Into<String>, arg_type: ArgType) -> Self {
        Self {
            name: name.into(),
            arg_type,
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

    pub fn required(mut self, name: impl Into<String>, arg_type: ArgType) -> Self {
        self.args.push(ArgSpec::required(name, arg_type));
        self
    }

    pub fn optional(mut self, name: impl Into<String>, arg_type: ArgType) -> Self {
        self.args.push(ArgSpec::optional(name, arg_type));
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

/// Introspection metadata for a registered handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandlerMetadata {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub args: ArgSchema,
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

    pub fn deterministic(mut self, deterministic: bool) -> Self {
        self.deterministic = deterministic;
        self
    }
}

fn default_deterministic() -> bool {
    true
}
