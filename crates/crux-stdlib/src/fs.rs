/// File-system handlers: read, write, glob, exists.
use crux_runtime::prelude::CruxErr;
use crux_script::{
    ArgSchema, ArgType, Capability, HandlerMetadata, HandlerRegistry, RiskLevel, SideEffect,
};
use serde_json::{Value, json};

use crate::error::require_str;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new("fs::read")
            .describe("Read the contents of a file from the filesystem.")
            .args(ArgSchema::new().required("path", ArgType::String))
            .risk(RiskLevel::Medium)
            .side_effects(vec![SideEffect::ReadFs])
            .capabilities(vec![Capability::ReadFs])
            .deterministic(false),
        |input: Value| async move {
            let path = require_str(&input, "path")?.to_string();
            let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
                CruxErr::step_failed("fs::read", format!("cannot read {path}: {e}"))
            })?;
            Ok(json!({ "content": content, "path": path }))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("fs::write")
            .describe("Write content to a file on the filesystem.")
            .args(
                ArgSchema::new()
                    .required("path", ArgType::String)
                    .optional("content", ArgType::Any),
            )
            .risk(RiskLevel::Medium)
            .side_effects(vec![SideEffect::WriteFs])
            .capabilities(vec![Capability::WriteFs])
            .deterministic(false),
        |input: Value| async move {
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
            tokio::fs::write(&path, &content).await.map_err(|e| {
                CruxErr::step_failed("fs::write", format!("cannot write {path}: {e}"))
            })?;
            Ok(json!({ "written": true, "path": path }))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("fs::glob")
            .describe("Expand a glob pattern and return matching filesystem paths.")
            .args(ArgSchema::new().required("pattern", ArgType::String))
            .risk(RiskLevel::Medium)
            .side_effects(vec![SideEffect::ReadFs])
            .capabilities(vec![Capability::ReadFs])
            .deterministic(false),
        |input: Value| async move {
            let pattern = require_str(&input, "pattern")?.to_string();
            let paths: Vec<Value> = glob::glob(&pattern)
                .map_err(|e| CruxErr::step_failed("fs::glob", format!("invalid pattern: {e}")))?
                .filter_map(|entry| entry.ok())
                .map(|p| Value::String(p.display().to_string()))
                .collect();
            Ok(json!({ "paths": paths }))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("fs::exists")
            .describe("Check whether a path exists on the filesystem.")
            .args(ArgSchema::new().required("path", ArgType::String))
            .risk(RiskLevel::Medium)
            .side_effects(vec![SideEffect::ReadFs])
            .capabilities(vec![Capability::ReadFs])
            .deterministic(false),
        |input: Value| async move {
            let path = require_str(&input, "path")?.to_string();
            let exists = tokio::fs::metadata(&path).await.is_ok();
            Ok(json!({ "exists": exists, "path": path }))
        },
    );
}
