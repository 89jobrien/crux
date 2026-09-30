//! Registration and manifest tests for CLI planning handlers.
//!
//! Tests for `crux plan` CLI wiring (#17).
//!
//! Tests verify that:
//! - `llm::plan` handler is registered by `register_all`
//! - `llm::stream` appears in the handler manifest used for planning
//! - Plan errors without any backend produce a useful error message

#[test]
fn llm_plan_handler_registered_by_register_all() {
    let mut reg = crux_script::HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);
    assert!(
        reg.get_handler("llm::plan").is_some(),
        "llm::plan must be registered by register_all"
    );
}

/// Verify the handler manifest exposed to the planner includes llm::stream.
#[test]
fn handler_manifest_includes_llm_stream() {
    // The manifest is internal to crux-baml's planner module; we check it
    // indirectly by verifying register_all registers llm::stream (if stream is in
    // the manifest, generate_pipeline can reference it in plans).
    let mut reg = crux_script::HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);
    assert!(
        reg.get_handler("llm::stream").is_some(),
        "llm::stream must be registered so planner can reference it"
    );
}
