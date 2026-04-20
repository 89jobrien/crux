/// Conformance test suite — port/adapter contract verification for cruxx.
///
/// Each submodule targets one hexagonal boundary:
///   conformance::registry_backend — RegistryBackend port, InMemoryBackend adapter
///   conformance::context_adapter  — Context port, CruxCtx adapter
///   conformance::agent_port       — Agent port, macro vs hand-written equivalence
///   conformance::llm_provider     — LlmProvider port, stub adapter
mod conformance {
    mod agent_port;
    mod context_adapter;
    mod llm_provider;
    mod registry_backend;
}
