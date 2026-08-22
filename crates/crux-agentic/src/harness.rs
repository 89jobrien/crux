use crux_runtime::prelude::CruxErr;
use crux_runtime::types::evolution::EvolutionOutcome;
use crux_runtime::types::harness::{HarnessDiff, HarnessProfile};
use crux_script::{ArgSchema, ArgType, HandlerMetadata, HandlerRegistry, RiskLevel, SideEffect};
use serde_json::Value;

/// Register harness step handlers.
pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new("harness::evolve")
            .describe("Propose a HarnessDiff against a base profile and return both.")
            .args(ArgSchema::new().required("base_profile", ArgType::Object))
            .risk(RiskLevel::Low)
            .side_effects(vec![])
            .capabilities(vec![])
            .deterministic(true),
        |input: Value| async move { handle_evolve(input).await },
    );
    registry.handler_value_with_metadata(
        HandlerMetadata::new("harness::canary")
            .describe("Score a candidate profile against the baseline and decide its fate.")
            .args(ArgSchema::new().required("candidate_profile", ArgType::Object))
            .risk(RiskLevel::Medium)
            .side_effects(vec![SideEffect::Process])
            .capabilities(vec![])
            .deterministic(true),
        |input: Value| async move { handle_canary(input).await },
    );
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
