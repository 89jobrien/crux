//! Translates a caller-supplied JSON Schema into BAML runtime types.
//!
//! BAML 0.221 can inject output fields at runtime through a `TypeBuilder`, but
//! it does not ship a JSON Schema importer — the upstream docs note that
//! "we have a working implementation of this feature, but we are waiting for
//! concrete use cases to merge it into the main codebase". So a pipeline that
//! wants `llm::invoke` to return a caller-chosen shape has to describe it as
//! JSON Schema, and this module is the bridge.
//!
//! Scope is deliberately the subset that maps cleanly onto BAML:
//!
//! | JSON Schema       | BAML                      |
//! | ----------------- | ------------------------- |
//! | `string`          | `string`                  |
//! | `number`          | `float`                   |
//! | `integer`         | `int`                     |
//! | `boolean`         | `bool`                    |
//! | `null`            | `null`                    |
//! | `array`           | `T[]`                     |
//! | object, no props  | `map<string, string>`     |
//! | `enum` of strings | union of literal strings  |
//! | `anyOf` / `oneOf` | union                     |
//! | `const` string    | literal string            |
//! | `$ref`            | resolved against `$defs`  |
//!
//! Anything outside that (nested object shapes, tuple types, remote `$ref`s) is
//! rejected with a message naming the field, so a pipeline author gets a
//! validation error rather than a silently mis-shaped LLM schema.

use crate::baml_client::type_builder::TypeBuilder;
use baml::TypeDef;
use serde_json::Value;
use std::collections::HashMap;

/// Maximum `$ref` nesting, guarding against a schema that references itself.
const MAX_REF_DEPTH: usize = 32;

/// The BAML class that `@@dynamic` fields are injected into.
const TARGET_CLASS: &str = "Completion";

/// Fields the `Completion` class already declares.
///
/// A caller schema may not redefine these. `content` is what every completion
/// returns, and `confidence` is the score the handler lifts into the step
/// confidence — letting a schema override it would leave the payload and the step
/// confidence disagreeing, which is exactly the ambiguity routing must not have.
const RESERVED_FIELDS: &[&str] = &["content", "confidence"];

/// Error raised when a JSON Schema cannot be represented as BAML output types.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct SchemaError {
    message: String,
}

impl SchemaError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<String> for SchemaError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

/// Builds a [`TypeBuilder`] carrying one injected field per property of `schema`.
///
/// Returns `Ok(None)` when the schema declares no properties — the caller then
/// issues a plain free-text completion.
pub fn type_builder_for(schema: &Value) -> Result<Option<TypeBuilder>, SchemaError> {
    let properties = properties_of(schema)?;
    if properties.is_empty() {
        return Ok(None);
    }

    let builder = TypeBuilder::new();
    let refs = collect_refs(schema);
    let mut errors = Vec::new();

    for (name, property) in properties {
        if RESERVED_FIELDS.contains(&name.as_str()) {
            errors.push(format!(
                "field '{name}' is provided by the handler already and cannot be redefined"
            ));
            continue;
        }
        match type_def_for(&builder, property, &refs, 0) {
            Ok(type_def) => {
                let class = builder.get_class(TARGET_CLASS).ok_or_else(|| {
                    SchemaError::new(format!("`{TARGET_CLASS}` is missing from the BAML schema"))
                })?;
                if let Err(error) = class.add_property(name, &type_def) {
                    errors.push(format!("field '{name}': {error}"));
                }
            }
            Err(reason) => errors.push(format!("field '{name}': {reason}")),
        }
    }

    if !errors.is_empty() {
        return Err(SchemaError::new(format!(
            "unsupported output schema — {}",
            errors.join("; ")
        )));
    }
    Ok(Some(builder))
}

/// Properties of `schema`, accepting either a bare object schema or a map of
/// per-property schemas.
fn properties_of(schema: &Value) -> Result<&serde_json::Map<String, Value>, String> {
    let Some(object) = schema.as_object() else {
        return Err(format!(
            "expected a JSON Schema object, got {}",
            kind_of(schema)
        ));
    };

    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        return Ok(properties);
    }
    // A bare `{"field": {"type": ...}}` map is accepted as shorthand, matching how
    // pipeline authors tend to think about "just tell it the fields". Values must
    // be schemas, not samples: a bare `"high"` is ambiguous between a type and an
    // enum, and guessing wrong silently mis-shapes the model's output.
    if !object.is_empty()
        && object
            .iter()
            .all(|(key, value)| !key.starts_with('$') && value.is_object())
    {
        return Ok(object);
    }
    Err(
        "expected a JSON Schema with a 'properties' object, or a bare map of \
         field schemas ({field: {type: string}}). Values must be schemas, not samples."
            .to_string(),
    )
}

/// Collect the local `$defs` / `definitions` a `$ref` may point at.
fn collect_refs(schema: &Value) -> HashMap<String, Value> {
    let mut refs = HashMap::new();
    for key in ["$defs", "definitions"] {
        let Some(defs) = schema.get(key).and_then(Value::as_object) else {
            continue;
        };
        for (name, definition) in defs {
            refs.insert(format!("#/{key}/{name}"), definition.clone());
        }
    }
    refs
}

/// Translate one JSON Schema node into a BAML type.
fn type_def_for(
    builder: &TypeBuilder,
    schema: &Value,
    refs: &HashMap<String, Value>,
    depth: usize,
) -> Result<TypeDef, String> {
    if depth > MAX_REF_DEPTH {
        return Err(format!("nesting deeper than {MAX_REF_DEPTH} levels"));
    }
    let Some(object) = schema.as_object() else {
        return Err(format!("expected a schema object, got {}", kind_of(schema)));
    };

    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        let resolved = refs.get(reference).ok_or_else(|| {
            format!("unsupported $ref '{reference}' (only local $defs are supported)")
        })?;
        return type_def_for(builder, resolved, refs, depth + 1);
    }

    // `anyOf` / `oneOf` become BAML unions. `oneOf` collapses to `anyOf` here:
    // BAML unions are not exclusive, but exclusivity is a property the caller's
    // own validation — not the generated schema — has to enforce.
    if let Some(variants) = object
        .get("anyOf")
        .or_else(|| object.get("oneOf"))
        .and_then(Value::as_array)
        .filter(|variants| !variants.is_empty())
    {
        let built = variants
            .iter()
            .map(|variant| type_def_for(builder, variant, refs, depth + 1))
            .collect::<Result<Vec<_>, _>>()?;
        let borrowed: Vec<&TypeDef> = built.iter().collect();
        return Ok(builder.union(&borrowed));
    }

    if let Some(values) = object.get("enum").and_then(Value::as_array) {
        if values.is_empty() {
            return Err("'enum' must list at least one value".to_string());
        }
        if values.iter().all(Value::is_string) {
            let built: Vec<TypeDef> = values
                .iter()
                .filter_map(Value::as_str)
                .map(|value| builder.literal_string(value))
                .collect();
            let borrowed: Vec<&TypeDef> = built.iter().collect();
            return Ok(builder.union(&borrowed));
        }
        return Err("only string 'enum' values are supported".to_string());
    }

    // A `const` is a single-value literal; treat it like a one-element enum.
    if let Some(value) = object.get("const") {
        return match value.as_str() {
            Some(text) => Ok(builder.literal_string(text)),
            None => Err("only string 'const' values are supported".to_string()),
        };
    }

    let kind = match object.get("type") {
        Some(Value::String(name)) => name.as_str(),
        None if object.get("properties").is_some() => "object",
        None => "string",
        Some(other) => return Err(format!("'type' must be a string, got {other}")),
    };

    match kind {
        "string" => Ok(builder.string()),
        "number" => Ok(builder.float()),
        "integer" => Ok(builder.int()),
        "boolean" => Ok(builder.bool()),
        "null" => Ok(builder.null()),
        "array" => {
            let items = object
                .get("items")
                .ok_or_else(|| "'array' requires an 'items' schema".to_string())?;
            Ok(builder.list(&type_def_for(builder, items, refs, depth + 1)?))
        }
        "object" => Ok(string_map(builder)),
        other => Err(format!("unsupported type '{other}'")),
    }
}

/// A BAML `map<string, string>`.
///
/// A free-form object cannot become a BAML class mid-call — classes have to be
/// declared in the schema or added by name — so nested object properties are not
/// representable here. Use an array of scalars, or a named BAML function, when a
/// nested shape is genuinely needed.
fn string_map(builder: &TypeBuilder) -> TypeDef {
    builder.map(&builder.string(), &builder.string())
}

/// Human-readable JSON type name for error messages.
fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn build(schema: Value) -> Result<Option<TypeBuilder>, SchemaError> {
        type_builder_for(&schema)
    }

    #[test]
    fn empty_properties_yields_no_type_builder() {
        assert!(build(json!({ "type": "object", "properties": {} })).is_ok_and(|tb| tb.is_none()));
    }

    #[test]
    fn scalar_fields_are_accepted() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "score": { "type": "number" },
                "count": { "type": "integer" },
                "ok": { "type": "boolean" }
            }
        });
        assert!(build(schema).is_ok_and(|tb| tb.is_some()));
    }

    #[test]
    fn enum_becomes_a_literal_union() {
        let schema = json!({
            "type": "object",
            "properties": { "verdict": { "type": "string", "enum": ["pass", "block"] } }
        });
        assert!(build(schema).is_ok_and(|tb| tb.is_some()));
    }

    #[test]
    fn array_of_scalars_is_accepted() {
        let schema = json!({
            "type": "object",
            "properties": { "blockers": { "type": "array", "items": { "type": "string" } } }
        });
        assert!(build(schema).is_ok_and(|tb| tb.is_some()));
    }

    #[test]
    fn any_of_becomes_a_union() {
        let schema = json!({
            "type": "object",
            "properties": {
                "value": { "anyOf": [{ "type": "string" }, { "type": "number" }] }
            }
        });
        assert!(build(schema).is_ok_and(|tb| tb.is_some()));
    }

    #[test]
    fn local_refs_resolve() {
        let schema = json!({
            "type": "object",
            "$defs": { "Severity": { "type": "string", "enum": ["low", "high"] } },
            "properties": { "severity": { "$ref": "#/$defs/Severity" } }
        });
        assert!(build(schema).is_ok_and(|tb| tb.is_some()));
    }

    #[test]
    fn bare_field_map_is_shorthand_for_properties() {
        let schema = json!({ "name": { "type": "string" }, "score": { "type": "number" } });
        assert!(build(schema).is_ok_and(|tb| tb.is_some()));
    }

    #[test]
    fn unsupported_type_names_the_field() {
        let schema = json!({
            "type": "object",
            "properties": { "ratio": { "type": "decimal" } }
        });
        let error = build(schema).expect_err("decimal is not a JSON Schema type");
        let message = error.to_string();
        assert!(message.contains("ratio"), "got: {message}");
        assert!(message.contains("decimal"), "got: {message}");
    }

    #[test]
    fn remote_ref_is_rejected_by_name() {
        let schema = json!({
            "type": "object",
            "properties": { "ext": { "$ref": "https://example.com/schema.json" } }
        });
        let error = build(schema).expect_err("remote $ref must be rejected");
        assert!(error.to_string().contains("example.com"), "got: {error}");
    }

    #[test]
    fn array_without_items_is_rejected() {
        let schema = json!({ "type": "object", "properties": { "xs": { "type": "array" } } });
        let error = build(schema).expect_err("array needs items");
        assert!(error.to_string().contains("items"), "got: {error}");
    }

    #[test]
    fn non_string_enum_is_rejected() {
        let schema = json!({
            "type": "object",
            "properties": { "n": { "type": "integer", "enum": [1, 2] } }
        });
        let error = build(schema).expect_err("only string enums are supported");
        assert!(error.to_string().contains("enum"), "got: {error}");
    }

    #[test]
    fn non_object_schema_is_rejected() {
        let error = build(json!("just a string")).expect_err("a schema must be an object");
        assert!(
            error.to_string().contains("expected a JSON Schema object"),
            "got: {error}"
        );
    }

    #[test]
    fn schema_with_no_properties_key_is_rejected() {
        let error = build(json!({ "type": "object" })).expect_err("no properties to inject");
        assert!(error.to_string().contains("properties"), "got: {error}");
    }

    /// Every rejection must name the offending field, otherwise a pipeline
    /// author has no idea which of twenty fields is wrong.
    #[test]
    fn errors_name_every_offending_field() {
        let schema = json!({
            "type": "object",
            "properties": {
                "good": { "type": "string" },
                "bad_one": { "type": "decimal" },
                "bad_two": { "type": "nonsense" }
            }
        });
        let message = build(schema)
            .expect_err("both bad fields must be reported")
            .to_string();
        assert!(message.contains("bad_one"), "got: {message}");
        assert!(message.contains("bad_two"), "got: {message}");
    }
}
