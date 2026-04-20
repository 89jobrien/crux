/// Conformance test suite — port/adapter contract verification.
///
/// Each module targets one hexagonal boundary:
///   registry_backend — RegistryBackend port, InMemoryBackend adapter
///   context_adapter  — Context port, CruxCtx adapter
///   agent_port       — Agent port, macro vs hand-written equivalence
///   llm_provider     — LlmProvider port, stub adapter
pub mod agent_port;
pub mod context_adapter;
pub mod llm_provider;
pub mod registry_backend;
