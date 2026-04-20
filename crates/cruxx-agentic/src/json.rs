use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use serde_json::{Map, Value};

use crate::error::require_str;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value("json::pick", |input: Value| async move {
        let fields: Vec<String> = input
            .get("args")
            .and_then(|a| a.get("fields"))
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        // Pick from the payload — the handler input object minus the injected `args` key.
        // This avoids mixing pipeline metadata (`args`) with actual payload fields.
        let mut out = Map::new();
        if let Value::Object(map) = &input {
            for field in &fields {
                if field == "args" {
                    continue; // never pick the `args` metadata key
                }
                if let Some(v) = map.get(field) {
                    out.insert(field.clone(), v.clone());
                }
            }
        }
        Ok(Value::Object(out))
    });

    registry.handler_value("json::merge", |input: Value| async move {
        let overlay = input
            .get("args")
            .and_then(|a| a.get("with"))
            .cloned()
            .unwrap_or(Value::Null);

        // Use the handler input as the merge base after stripping the injected `args` key.
        // `args` is pipeline metadata (static step config) and is intentionally excluded from the
        // merged output so it does not leak into downstream steps as a data field.
        let mut base = match input {
            Value::Object(mut m) => {
                m.remove("args");
                m
            }
            _ => Map::new(),
        };

        if let Value::Object(over) = overlay {
            for (k, v) in over {
                base.insert(k, v);
            }
        }
        Ok(Value::Object(base))
    });

    registry.handler_value("json::jq", |input: Value| async move {
        let expr = require_str(&input, "expr")
            .map_err(CruxErr::from)?
            .to_string();

        // Strip the injected `args` key before evaluating so expressions operate
        // on the actual pipeline payload.
        let payload = match input.clone() {
            Value::Object(mut m) => {
                m.remove("args");
                Value::Object(m)
            }
            other => other,
        };

        let result = eval_jq(&payload, expr.trim())?;
        Ok(result)
    });
}

/// Evaluate a limited subset of jq expressions against `value`.
///
/// Supported forms:
/// - `.`                        — identity
/// - `.field`                   — field access
/// - `.field.nested`            — nested field access
/// - `.field.[N]`               — array index
/// - `keys`                     — sorted object keys (excluding `args`)
/// - `<expr> | length`          — array / string / object length
/// - `<expr> | type`            — JSON type name
/// - `<expr> | first`           — first array element
/// - `<expr> | last`            — last array element
/// - `has("key")`               — boolean key existence check
///
/// For complex queries use `shell::capture` with the `jq` binary.
fn eval_jq(value: &Value, expr: &str) -> Result<Value, CruxErr> {
    // Pipe: evaluate left, pass result to right.
    if let Some(pipe_pos) = find_pipe(expr) {
        let left = expr[..pipe_pos].trim();
        let right = expr[pipe_pos + 1..].trim();
        let intermediate = eval_jq(value, left)?;
        return eval_jq(&intermediate, right);
    }

    // `keys` — sorted object keys of the current value.
    if expr == "keys" {
        let keys = match value {
            Value::Object(m) => {
                let mut ks: Vec<Value> = m.keys().map(|k| Value::String(k.clone())).collect();
                ks.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                ks
            }
            _ => vec![],
        };
        return Ok(Value::Array(keys));
    }

    // `length` — length of array, string, or object.
    if expr == "length" {
        return Ok(match value {
            Value::Array(a) => Value::Number(a.len().into()),
            Value::String(s) => Value::Number(s.len().into()),
            Value::Object(m) => Value::Number(m.len().into()),
            Value::Null => Value::Number(0.into()),
            _ => Value::Number(1.into()),
        });
    }

    // `type` — JSON type name.
    if expr == "type" {
        let t = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        return Ok(Value::String(t.into()));
    }

    // `first` — first element of an array.
    if expr == "first" {
        return Ok(match value {
            Value::Array(a) => a.first().cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        });
    }

    // `last` — last element of an array.
    if expr == "last" {
        return Ok(match value {
            Value::Array(a) => a.last().cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        });
    }

    // `has("key")` — boolean key existence.
    if let Some(key) = parse_has(expr) {
        return Ok(Value::Bool(
            value.as_object().map(|m| m.contains_key(key)).unwrap_or(false),
        ));
    }

    // Path traversal: `.`, `.field`, `.field.nested`, `.field.[N]`
    let path = expr.trim_start_matches('.');
    Ok(traverse(value, path))
}

/// Find the first `|` that is not inside parentheses or quotes.
fn find_pipe(expr: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => in_str = !in_str,
            '(' if !in_str => depth += 1,
            ')' if !in_str => depth -= 1,
            '|' if !in_str && depth == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Parse `has("key")` → `Some("key")` or `None`.
fn parse_has(expr: &str) -> Option<&str> {
    let inner = expr.strip_prefix("has(")?.strip_suffix(')')?;
    let key = inner.trim().strip_prefix('"')?.strip_suffix('"')?;
    Some(key)
}

/// Minimal path traversal: `"foo.bar.[2]"` → nested field/index access.
///
/// Segments are split on `.`. Bracket segments like `[N]` are parsed as array
/// indices. Any missing key or out-of-bounds index returns `Value::Null`.
fn traverse(value: &Value, path: &str) -> Value {
    if path.is_empty() {
        return value.clone();
    }

    let (head, rest) = match path.find('.') {
        Some(i) => (&path[..i], path[i + 1..].trim_start_matches('.')),
        None => (path, ""),
    };

    let next = if head.starts_with('[') && head.ends_with(']') {
        let idx: usize = head[1..head.len() - 1].parse().unwrap_or(usize::MAX);
        value.get(idx).cloned().unwrap_or(Value::Null)
    } else {
        value.get(head).cloned().unwrap_or(Value::Null)
    };

    if rest.is_empty() {
        next
    } else {
        traverse(&next, rest)
    }
}
