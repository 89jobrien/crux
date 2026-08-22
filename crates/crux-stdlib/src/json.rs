use crux_runtime::prelude::CruxErr;
use crux_script::{ArgSchema, ArgType, HandlerMetadata, HandlerRegistry, RiskLevel};
use serde_json::{Map, Value, json};

use crate::error::require_str;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new("json::pick")
            .describe("Pick a subset of fields from the input object.")
            .args(ArgSchema::new().optional("fields", ArgType::Array))
            .risk(RiskLevel::Low)
            .side_effects(vec![])
            .capabilities(vec![])
            .deterministic(true),
        |input: Value| async move {
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
                    if field == "args" {
                        continue;
                    }
                    if let Some(v) = map.get(field) {
                        out.insert(field.clone(), v.clone());
                    }
                }
            }
            Ok(Value::Object(out))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("json::merge")
            .describe("Merge a static overlay object into the input.")
            .args(ArgSchema::new().optional("with", ArgType::Object))
            .risk(RiskLevel::Low)
            .side_effects(vec![])
            .capabilities(vec![])
            .deterministic(true),
        |input: Value| async move {
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
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("json::group_by")
            .describe("Group an array of objects by a shared key field.")
            .args(ArgSchema::new().optional("key", ArgType::String))
            .risk(RiskLevel::Low)
            .side_effects(vec![])
            .capabilities(vec![])
            .deterministic(true),
        |input: Value| async move {
            let key = input
                .get("args")
                .and_then(|a| a.get("key"))
                .and_then(|k| k.as_str())
                .unwrap_or("group");
            let items = input
                .get("items")
                .or_else(|| input.get("findings"))
                .or_else(|| input.get("todos"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut groups: std::collections::HashMap<String, Vec<Value>> =
                std::collections::HashMap::new();
            for item in items {
                let bucket = item
                    .get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                groups.entry(bucket).or_default().push(item);
            }

            let map: Map<String, Value> = groups
                .into_iter()
                .map(|(k, v)| (k, Value::Array(v)))
                .collect();
            Ok(Value::Object(map))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("json::filter_nonempty")
            .describe("Filter an array to items where a given field is non-empty.")
            .args(ArgSchema::new().optional("field", ArgType::String))
            .risk(RiskLevel::Low)
            .side_effects(vec![])
            .capabilities(vec![])
            .deterministic(true),
        |input: Value| async move {
            let field = input
                .get("args")
                .and_then(|a| a.get("field"))
                .and_then(|f| f.as_str())
                .unwrap_or("output");
            let items = input
                .get("items")
                .or_else(|| input.get("results"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let filtered: Vec<Value> = items
                .into_iter()
                .filter(|item| {
                    let val = item.get(field);
                    match val {
                        None | Some(Value::Null) => false,
                        Some(Value::String(s)) => !s.is_empty(),
                        Some(Value::Array(a)) => !a.is_empty(),
                        Some(Value::Object(m)) => !m.is_empty(),
                        _ => true,
                    }
                })
                .collect();

            Ok(json!({"items": filtered}))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("json::jq")
            .describe("Evaluate a limited jq-style expression against the input.")
            .args(ArgSchema::new().required("expr", ArgType::String))
            .risk(RiskLevel::Low)
            .side_effects(vec![])
            .capabilities(vec![])
            .deterministic(true),
        |input: Value| async move {
            let expr = require_str(&input, "expr")
                .map_err(CruxErr::from)?
                .to_string();

            let payload = match input.clone() {
                Value::Object(mut m) => {
                    m.remove("args");
                    Value::Object(m)
                }
                other => other,
            };

            let result = eval_jq(&payload, expr.trim())?;
            Ok(result)
        },
    );
}

const UNSUPPORTED_PREFIXES: &[&str] = &[
    "map_values(",
    "reduce ",
    "env",
    "path(",
    "recurse",
    "limit(",
    "until(",
    "any(",
    "all(",
    "indices(",
    "inside(",
    "contains(",
    "input",
    "debug",
    "error",
    "halt",
    "ascii_downcase",
    "ascii_upcase",
    "tostring",
    "tonumber",
    "tojson",
    "fromjson",
    "ltrimstr(",
    "rtrimstr(",
    "startswith(",
    "endswith(",
    "split(",
    "join(",
    "test(",
    "match(",
    "capture(",
    "scan(",
    "sub(",
    "gsub(",
    "explode",
    "implode",
    "ascii",
    "nan",
    "infinite",
    "isinfinite",
    "isnan",
    "isnormal",
    "isfinite",
    "sort_by(",
    "group_by(",
    "unique_by(",
    "min_by(",
    "max_by(",
    "to_entries",
    "from_entries",
    "with_entries(",
    "transpose",
    "input",
    "inputs",
    "empty",
    "add",
    "any",
    "all",
    "flatten",
    "range(",
    "floor",
    "round",
    "ceil",
    "sqrt",
    "pow(",
    "log",
    "fabs",
    "not",
    "recurse_down",
    "walk(",
    "env.",
    "@base32",
    "@base64",
    "@csv",
    "@html",
    "@json",
    "@sh",
    "@text",
    "@tsv",
    "@uri",
    "label-",
    "$__loc__",
    "builtins",
    "paths",
    "leaf_paths",
    "getpath(",
    "setpath(",
    "delpaths(",
    "isvalid",
    "modulemeta",
    "stderr",
    "input",
    "debug(",
    "indices(",
    "index(",
    "rindex(",
    "foreach",
    "try ",
    "if ",
    "def ",
    "import ",
    "include ",
];

fn is_unsupported(expr: &str) -> bool {
    if expr.starts_with('[') || expr.starts_with('{') {
        return true;
    }
    UNSUPPORTED_PREFIXES
        .iter()
        .any(|p| expr.starts_with(p) || expr == p.trim_end_matches('('))
}

fn eval_jq(value: &Value, expr: &str) -> Result<Value, CruxErr> {
    if is_unsupported(expr) {
        return Err(CruxErr::step_failed(
            "json::jq",
            format!(
                "json::jq only supports dot-path traversal and a limited set of \
                 built-ins (keys, length, type, first, last, has). \
                 Expression '{expr}' requires a full jq runtime. \
                 For complex queries use shell::capture with jq."
            ),
        ));
    }

    if let Some(pipe_pos) = find_pipe(expr) {
        let left = expr[..pipe_pos].trim();
        let right = expr[pipe_pos + 1..].trim();
        let intermediate = eval_jq(value, left)?;
        return eval_jq(&intermediate, right);
    }

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

    if expr == "length" {
        return Ok(match value {
            Value::Array(a) => Value::Number(a.len().into()),
            Value::String(s) => Value::Number(s.len().into()),
            Value::Object(m) => Value::Number(m.len().into()),
            Value::Null => Value::Number(0.into()),
            _ => Value::Number(1.into()),
        });
    }

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

    if expr == "first" {
        return Ok(match value {
            Value::Array(a) => a.first().cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        });
    }

    if expr == "last" {
        return Ok(match value {
            Value::Array(a) => a.last().cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        });
    }

    if let Some(key) = parse_has(expr) {
        return Ok(Value::Bool(
            value
                .as_object()
                .map(|m| m.contains_key(key))
                .unwrap_or(false),
        ));
    }

    if let Some(cond) = parse_call(expr, "select") {
        return eval_select(value, cond);
    }

    if let Some(inner) = parse_call(expr, "map") {
        return eval_map(value, inner);
    }

    let path = expr.trim_start_matches('.');
    Ok(traverse(value, path))
}

/// Extract the argument of a `name(...)` call, requiring the whole expression
/// to be exactly that call (balanced parens, nothing trailing).
fn parse_call<'a>(expr: &'a str, name: &str) -> Option<&'a str> {
    let rest = expr.strip_prefix(name)?.strip_prefix('(')?;
    let mut depth = 1i32;
    let mut in_str = false;
    for (byte_pos, c) in rest.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '(' if !in_str => depth += 1,
            ')' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    // the closing paren must be the last character of expr
                    return if byte_pos + c.len_utf8() == rest.len() {
                        Some(&rest[..byte_pos])
                    } else {
                        None
                    };
                }
            }
            _ => {}
        }
    }
    None
}

/// jq-style truthiness: everything except `null` and `false` is truthy.
fn is_truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false))
}

const COMPARISON_OPS: &[&str] = &["==", "!=", ">=", "<=", ">", "<"];

fn find_top_level_op<'a>(expr: &str, ops: &[&'a str]) -> Option<(usize, &'a str)> {
    let mut depth = 0i32;
    let mut in_str = false;
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => in_str = !in_str,
            '(' if !in_str => depth += 1,
            ')' if !in_str => depth -= 1,
            _ if !in_str && depth == 0 => {
                for op in ops {
                    if expr[i..].starts_with(op) {
                        return Some((i, op));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_literal(lit: &str) -> Value {
    let lit = lit.trim();
    serde_json::from_str(lit).unwrap_or_else(|_| Value::String(lit.to_string()))
}

fn compare_values(op: &str, lhs: &Value, rhs: &Value) -> bool {
    match op {
        "==" => lhs == rhs,
        "!=" => lhs != rhs,
        _ => {
            let ordering = match (lhs, rhs) {
                (Value::Number(a), Value::Number(b)) => a.as_f64().partial_cmp(&b.as_f64()),
                (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
                _ => None,
            };
            match (op, ordering) {
                (">", Some(o)) => o.is_gt(),
                ("<", Some(o)) => o.is_lt(),
                (">=", Some(o)) => o.is_ge(),
                ("<=", Some(o)) => o.is_le(),
                _ => false,
            }
        }
    }
}

/// Evaluate a boolean condition (used by `select(...)`) against a single value.
fn eval_condition(item: &Value, cond: &str) -> Result<bool, CruxErr> {
    let cond = cond.trim();
    if let Some((pos, op)) = find_top_level_op(cond, COMPARISON_OPS) {
        let lhs_expr = cond[..pos].trim();
        let rhs_expr = cond[pos + op.len()..].trim();
        let lhs = eval_jq(item, lhs_expr)?;
        let rhs = if rhs_expr.starts_with('.') {
            eval_jq(item, rhs_expr)?
        } else {
            parse_literal(rhs_expr)
        };
        return Ok(compare_values(op, &lhs, &rhs));
    }

    let result = eval_jq(item, cond)?;
    Ok(is_truthy(&result))
}

fn eval_select(value: &Value, cond: &str) -> Result<Value, CruxErr> {
    match value {
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                if eval_condition(item, cond)? {
                    out.push(item.clone());
                }
            }
            Ok(Value::Array(out))
        }
        other => {
            if eval_condition(other, cond)? {
                Ok(other.clone())
            } else {
                Ok(Value::Null)
            }
        }
    }
}

fn eval_map(value: &Value, expr: &str) -> Result<Value, CruxErr> {
    match value {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(eval_jq(item, expr.trim())?);
            }
            Ok(Value::Array(out))
        }
        other => Err(CruxErr::step_failed(
            "json::jq",
            format!("map(...) requires an array input, got {}", type_name(other)),
        )),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

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

fn parse_has(expr: &str) -> Option<&str> {
    let inner = expr.strip_prefix("has(")?.strip_suffix(')')?;
    let key = inner.trim().strip_prefix('"')?.strip_suffix('"')?;
    Some(key)
}

#[derive(Debug, Clone)]
enum PathSeg {
    Key(String),
    Index(usize),
}

/// Split a jq-style path (e.g. `foo.bar[2].baz` or `foo[0][1]`) into an
/// ordered list of key/index segments.
fn parse_path_segments(path: &str) -> Vec<PathSeg> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !current.is_empty() {
                    segments.push(PathSeg::Key(std::mem::take(&mut current)));
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(PathSeg::Key(std::mem::take(&mut current)));
                }
                let mut idx = String::new();
                for ic in chars.by_ref() {
                    if ic == ']' {
                        break;
                    }
                    idx.push(ic);
                }
                if let Ok(n) = idx.trim().parse::<usize>() {
                    segments.push(PathSeg::Index(n));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        segments.push(PathSeg::Key(current));
    }
    segments
}

fn traverse(value: &Value, path: &str) -> Value {
    if path.is_empty() {
        return value.clone();
    }

    let mut current = value.clone();
    for seg in parse_path_segments(path) {
        current = match seg {
            PathSeg::Key(k) => current.get(&k).cloned().unwrap_or(Value::Null),
            PathSeg::Index(i) => current.get(i).cloned().unwrap_or(Value::Null),
        };
    }
    current
}
