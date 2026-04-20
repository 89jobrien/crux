use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use serde_json::Value;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler("ctrl::noop", |input: Value| async move { Ok(input) });

    registry.handler("ctrl::log", |input: Value| async move {
        eprintln!(
            "[cruxx::ctrl::log] {}",
            serde_json::to_string(&input).unwrap_or_default()
        );
        Ok(input)
    });

    registry.handler("ctrl::assert", |input: Value| async move {
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
    });
}
