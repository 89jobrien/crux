use crux_runtime::prelude::CruxErr;
use crux_script::{ArgSchema, ArgType, HandlerMetadata, HandlerRegistry};
use serde_json::Value;

/// If `v` is a shell result object (`{exit_code, stdout, ...}`), return the
/// stdout string. If `v` is an array of shell results, return an array of
/// their stdout values. Otherwise return `v` unchanged.
fn unwrap_shell_results(v: Value) -> Value {
    fn is_shell_result(obj: &serde_json::Map<String, Value>) -> bool {
        obj.contains_key("exit_code") && obj.contains_key("stdout")
    }

    fn extract_stdout(obj: &serde_json::Map<String, Value>) -> Value {
        obj.get("stdout").cloned().unwrap_or(Value::Null)
    }

    match v {
        Value::Object(obj) if is_shell_result(&obj) => extract_stdout(&obj),
        Value::Object(obj) => {
            let unwrapped: serde_json::Map<String, Value> = obj
                .into_iter()
                .map(|(k, val)| (k, unwrap_shell_results(val)))
                .collect();
            Value::Object(unwrapped)
        }
        Value::Array(items) => {
            let unwrapped: Vec<Value> = items.into_iter().map(unwrap_shell_results).collect();
            Value::Array(unwrapped)
        }
        _ => v,
    }
}

/// Render a JSON value as plaintext: strings unquoted, arrays newline-joined,
/// objects as pretty JSON.
fn to_plaintext(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(items) => items
            .iter()
            .map(to_plaintext)
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => serde_json::to_string_pretty(v).unwrap_or_default(),
    }
}

/// Like `to_plaintext` but adds visual separation between array elements and
/// always pretty-prints objects.
fn to_plaintext_pretty(v: &Value) -> String {
    match v {
        Value::Array(items) => items
            .iter()
            .map(to_plaintext_pretty)
            .collect::<Vec<_>>()
            .join("---\n"),
        Value::Object(_) => serde_json::to_string_pretty(v).unwrap_or_default(),
        other => to_plaintext(other),
    }
}

/// Register a `ctrl::echo` agent that returns its input unchanged.
/// Useful for testing delegation in pipelines.
pub fn register_echo_agent(registry: &mut HandlerRegistry) {
    registry.agent_fn("echo", |input: Value| async move { Ok(input) });
}

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new("ctrl::noop").describe("Pass input through unchanged"),
        |input: Value| async move { Ok(input) },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("ctrl::log")
            .describe("Log input to stderr and pass through unchanged")
            .deterministic(false)
            .args(
                ArgSchema::new()
                    .optional("field", ArgType::String)
                    .optional("compact", ArgType::Boolean)
                    .optional("pretty", ArgType::Boolean),
            ),
        |input: Value| async move {
            let args = input.get("args");
            let field_name = args.and_then(|a| a.get("field")).and_then(|v| v.as_str());
            let compact = args
                .and_then(|a| a.get("compact"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let pretty = args
                .and_then(|a| a.get("pretty"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let display_value = if let Some(field) = field_name {
                input.get(field).unwrap_or(&Value::Null).clone()
            } else {
                // Strip internal keys so only pipeline data is logged.
                let mut v = input.clone();
                if let Some(obj) = v.as_object_mut() {
                    obj.remove("args");
                }
                v
            };

            let display_value = unwrap_shell_results(display_value);

            let formatted = if compact {
                to_plaintext(&display_value).trim().to_string()
            } else if pretty {
                to_plaintext_pretty(&display_value)
            } else {
                to_plaintext(&display_value)
            };

            eprintln!("[crux::ctrl::log] {formatted}");
            Ok(input)
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("ctrl::assert")
            .describe("Assert a condition is truthy; fail with message if not")
            .args(
                ArgSchema::new()
                    .optional("condition", ArgType::Any)
                    .optional("message", ArgType::String),
            ),
        |input: Value| async move {
            let condition = input
                .get("args")
                .and_then(|a| a.get("condition"))
                .unwrap_or(&Value::Null);

            let ok = match condition {
                Value::Bool(b) => *b,
                Value::Null => false,
                Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
                Value::String(s) => !s.is_empty(),
                Value::Array(a) => !a.is_empty(),
                Value::Object(o) => !o.is_empty(),
            };

            if ok {
                Ok(input)
            } else {
                let msg = input
                    .get("args")
                    .and_then(|a| a.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("assertion failed");
                Err(CruxErr::step_failed("ctrl::assert", msg))
            }
        },
    );
}
