//! Provider-neutral secret references, resolution, and trace redaction.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::recorder::Redactor;
use crate::types::error::CruxErr;

/// A provider-neutral reference to a secret value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRef {
    #[serde(rename = "$secret")]
    key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
}

impl SecretRef {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            provider: None,
        }
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
}

/// Resolved secret material that redacts its debug representation.
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Deliberately expose the secret for injection into a handler invocation.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue(***REDACTED***)")
    }
}

/// Port implemented by environment, vault, or platform-specific secret providers.
pub trait SecretResolver: Send + Sync {
    fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, CruxErr>;
}

/// Redacts every resolved secret before step outputs or errors enter a trace.
#[derive(Debug, Default)]
pub struct SecretRedactor {
    values: Vec<String>,
}

impl SecretRedactor {
    pub const fn new() -> Self {
        Self { values: Vec::new() }
    }

    pub fn register(&mut self, secret: &SecretValue) {
        if !secret.0.is_empty() && !self.values.contains(&secret.0) {
            self.values.push(secret.0.clone());
        }
    }

    fn redact_string(&self, mut value: String) -> String {
        for secret in &self.values {
            value = value.replace(secret, "***");
        }
        value
    }

    fn redact_value(&self, value: Value) -> Value {
        match value {
            Value::String(value) => Value::String(self.redact_string(value)),
            Value::Array(values) => Value::Array(
                values
                    .into_iter()
                    .map(|value| self.redact_value(value))
                    .collect(),
            ),
            Value::Object(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, self.redact_value(value)))
                    .collect(),
            ),
            value => value,
        }
    }
}

impl Redactor for SecretRedactor {
    fn redact_output(&self, output: Value) -> Value {
        self.redact_value(output)
    }

    fn redact_error(&self, error: &str) -> String {
        self.redact_string(error.to_owned())
    }
}

/// Resolve `{"$secret": "name", "provider": "..."}` objects recursively.
///
/// Each resolved value is registered with `redactor`; attach it to `CruxCtx` before
/// executing handlers so persisted step outputs and errors cannot contain the value.
pub fn resolve_secret_refs(
    value: Value,
    resolver: &dyn SecretResolver,
    redactor: &mut SecretRedactor,
) -> Result<Value, CruxErr> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|value| resolve_secret_refs(value, resolver, redactor))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) if values.contains_key("$secret") => {
            let reference: SecretRef = serde_json::from_value(Value::Object(values))
                .map_err(|error| CruxErr::step_failed("secret", error.to_string()))?;
            let secret = resolver.resolve(&reference)?;
            redactor.register(&secret);
            Ok(Value::String(secret.expose_secret().to_owned()))
        }
        Value::Object(values) => values
            .into_iter()
            .map(|(key, value)| {
                resolve_secret_refs(value, resolver, redactor).map(|value| (key, value))
            })
            .collect::<Result<serde_json::Map<_, _>, _>>()
            .map(Value::Object),
        value => Ok(value),
    }
}
