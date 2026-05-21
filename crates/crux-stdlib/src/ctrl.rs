use cruxx_core::prelude::CruxErr;
use cruxx_script::{ArgSchema, ArgType, HandlerMetadata, HandlerRegistry};
use serde_json::Value;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new("ctrl::noop").describe("Pass input through unchanged"),
        |input: Value| async move { Ok(input) },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("ctrl::log")
            .describe("Log input to stderr and pass through unchanged")
            .deterministic(false),
        |input: Value| async move {
            eprintln!(
                "[cruxx::ctrl::log] {}",
                serde_json::to_string(&input).unwrap_or_default()
            );
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
