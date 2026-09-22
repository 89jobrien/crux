//! Trace export adapters for post-hoc analysis and tracing subscribers.

use serde::Serialize;

use crate::types::crux_value::Crux;

#[derive(Serialize)]
struct StepSpan<'a> {
    trace_id: String,
    agent: &'a str,
    step: &'a str,
    stable_id: Option<&'a str>,
    kind: crate::types::step::StepKind,
    status: crate::types::step::StepStatus,
    confidence: f32,
    started_at: chrono::DateTime<chrono::Utc>,
    duration_ms: u64,
    metadata: &'a std::collections::HashMap<String, serde_json::Value>,
}

/// Serialize every trace step as one structured JSONL span record.
pub fn trace_to_jsonl<T>(trace: &Crux<T>) -> Result<String, serde_json::Error> {
    let mut output = String::new();
    for step in &trace.steps {
        let span = StepSpan {
            trace_id: trace.id.to_string(),
            agent: &trace.agent,
            step: &step.name,
            stable_id: step.stable_id.as_deref(),
            kind: step.kind,
            status: step.status,
            confidence: step.confidence,
            started_at: step.started_at,
            duration_ms: step.duration_ms,
            metadata: &step.metadata,
        };
        output.push_str(&serde_json::to_string(&span)?);
        output.push('\n');
    }
    Ok(output)
}

/// Emit every trace step as a tracing span consumable by OpenTelemetry layers.
#[cfg(feature = "tracing")]
pub fn emit_trace_spans<T>(trace: &Crux<T>) {
    for step in &trace.steps {
        let span = tracing::info_span!(
            "crux.step",
            trace_id = %trace.id,
            agent = %trace.agent,
            step = %step.name,
            status = ?step.status,
            duration_ms = step.duration_ms,
            confidence = step.confidence,
            metadata = ?step.metadata,
        );
        let _entered = span.enter();
        tracing::info!("step.export");
    }
}
