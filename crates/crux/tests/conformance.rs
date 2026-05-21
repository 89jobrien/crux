/// Conformance test suite — port/adapter contract verification for crux.
///
/// Each submodule targets one hexagonal boundary:
///   conformance::registry_backend — RegistryBackend port, InMemoryBackend adapter
///   conformance::context_adapter  — Context port, CruxCtx adapter
///   conformance::agent_port       — Agent port, macro vs hand-written equivalence
///   conformance::llm_provider     — LlmProvider port, stub adapter
///   conformance::safety_policy    — SafetyPolicy port, BoundedPolicy adapter
///   conformance::approval_gate    — ApprovalGate port, AlwaysApprove/AlwaysDeny adapters
mod conformance {
    mod agent_port;
    mod approval_gate;
    mod context_adapter;
    mod llm_provider;
    mod registry_backend;
    mod safety_policy;
}
