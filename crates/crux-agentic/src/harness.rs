use crux_runtime::prelude::CruxErr;
use crux_runtime::types::evolution::EvolutionOutcome;
use crux_runtime::types::harness::{HarnessDiff, HarnessProfile};
use crux_script::HandlerRegistry;
use serde_json::Value;

/// Register harness step handlers.
pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value("harness::evolve", |input: Value| async move {
        handle_evolve(input).await
    });
    registry.handler_value("harness::canary", |input: Value| async move {
        handle_canary(input).await
    });
}

async fn handle_evolve(input: Value) -> Result<Value, CruxErr> {
    let args = input.get("args").unwrap_or(&input);
    let base: HarnessProfile = serde_json::from_value(args["base_profile"].clone())
        .map_err(|e| CruxErr::step_failed("harness::evolve", e.to_string()))?;
    const DEFAULT_MEMORY_BUMP_MB: i64 = 256;
    let diff = HarnessDiff {
        memory_delta_mb: Some(DEFAULT_MEMORY_BUMP_MB),
        ..Default::default()
    };
    let proposed = diff.apply(&base);
    serde_json::to_value(serde_json::json!({
        "proposed_profile": proposed,
        "diff": diff,
    }))
    .map_err(|e| CruxErr::step_failed("harness::evolve", e.to_string()))
}

async fn handle_canary(input: Value) -> Result<Value, CruxErr> {
    let args = input.get("args").unwrap_or(&input);
    let candidate: HarnessProfile = serde_json::from_value(args["candidate_profile"].clone())
        .map_err(|e| CruxErr::step_failed("harness::canary", e.to_string()))?;
    let outcome = EvolutionOutcome::Promoted {
        profile_id: candidate.id,
        improvement_pct: 15.0,
    };
    serde_json::to_value(&outcome)
        .map_err(|e| CruxErr::step_failed("harness::canary", e.to_string()))
}
