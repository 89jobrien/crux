/// File-system handlers: read, write, glob, exists.
use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use serde_json::{Value, json};

use crate::error::require_str;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value("fs::read", |input: Value| async move {
        let path = require_str(&input, "path")?.to_string();
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| CruxErr::step_failed("fs::read", format!("cannot read {path}: {e}")))?;
        Ok(json!({ "content": content, "path": path }))
    });

    registry.handler_value("fs::write", |input: Value| async move {
        let path = require_str(&input, "path")?.to_string();
        let content_val = input
            .get("args")
            .and_then(|a| a.get("content"))
            .ok_or_else(|| CruxErr::step_failed("fs::write", "missing arg: content"))?;
        let content = match content_val {
            Value::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other)
                .map_err(|e| CruxErr::step_failed("fs::write", format!("serialize: {e}")))?,
        };
        tokio::fs::write(&path, &content)
            .await
            .map_err(|e| CruxErr::step_failed("fs::write", format!("cannot write {path}: {e}")))?;
        Ok(json!({ "written": true, "path": path }))
    });

    registry.handler_value("fs::glob", |input: Value| async move {
        let pattern = require_str(&input, "pattern")?.to_string();
        let paths: Vec<Value> = glob::glob(&pattern)
            .map_err(|e| CruxErr::step_failed("fs::glob", format!("invalid pattern: {e}")))?
            .filter_map(|entry| entry.ok())
            .map(|p| Value::String(p.display().to_string()))
            .collect();
        Ok(json!({ "paths": paths }))
    });

    registry.handler_value("fs::exists", |input: Value| async move {
        let path = require_str(&input, "path")?.to_string();
        let exists = tokio::fs::metadata(&path).await.is_ok();
        Ok(json!({ "exists": exists, "path": path }))
    });
}
