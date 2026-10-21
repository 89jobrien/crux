//! Trace export adapters for post-hoc analysis and tracing subscribers.

use crux_types::emission::{Emission, RuntimeEvent};

use crate::types::crux_value::Crux;

/// Serialize every trace step as one structured JSONL span record.
pub fn trace_to_jsonl<T>(trace: &Crux<T>) -> Result<String, serde_json::Error> {
    let mut output = String::new();
    for (sequence, step) in trace.steps.iter().enumerate() {
        let event = RuntimeEvent {
            sequence: sequence as u64,
            emitted_at: step.started_at,
            trace_id: Some(trace.id.clone()),
            agent: Some(trace.agent.clone()),
            emission: Emission::StepRecorded {
                name: step.name.clone(),
                stable_id: step.stable_id.clone(),
                step_kind: step.kind,
                status: step.status,
                origin: step.origin,
                confidence: step.confidence,
                started_at: step.started_at,
                duration_ms: step.duration_ms,
                metadata: step.metadata.clone(),
            },
        };
        output.push_str(&serde_json::to_string(&event)?);
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
            origin = ?step.origin,
            duration_ms = step.duration_ms,
            confidence = step.confidence,
            metadata = ?step.metadata,
        );
        let _entered = span.enter();
        tracing::info!("step.export");
    }
}
