use cruxai_core::prelude::CruxErr;
use cruxai_script::HandlerRegistry;
use serde_json::{Map, Value};

use crate::error::require_str;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler("json::pick", |input: Value| async move {
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

        let mut out = Map::new();
        if let Value::Object(map) = &input {
            for field in &fields {
                if let Some(v) = map.get(field) {
                    out.insert(field.clone(), v.clone());
                }
            }
        }
        Ok(Value::Object(out))
    });

    registry.handler("json::merge", |input: Value| async move {
        let overlay = input
            .get("args")
            .and_then(|a| a.get("with"))
            .cloned()
            .unwrap_or(Value::Null);

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

    registry.handler("json::jq", |input: Value| async move {
        let expr = require_str(&input, "expr")
            .map_err(CruxErr::from)?
            .to_string();
        let path = expr.trim_start_matches('.');
        let result = traverse(&input, path);
        Ok(result)
    });
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
