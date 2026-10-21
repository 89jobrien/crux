//! LLM provider plumbing for crux-agentic.
//!
//! The pipeline handlers that make LLM calls — `llm::invoke`,
//! `llm::invoke_with_fallback`, and `llm::stream` — are **not** defined here.
//! They live in `crux-baml`, which routes every call through BAML so provider
//! selection, retries, and output parsing are handled in one place and
//! structured output is available to all of them. See [`crux_baml::complete`].
//!
//! What remains is the [`LlmProvider`](crate::provider::LlmProvider) port and its
//! adapters: the public seam for calling an LLM directly from Rust, used by
//! `llm_step` for agent-level completions and implemented by user code against
//! the conformance tests in `crux/tests/conformance`.

pub use crate::adapters::{AnthropicAdapter, OllamaAdapter, OpenAiAdapter};
