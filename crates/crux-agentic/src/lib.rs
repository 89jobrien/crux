//! crux-agentic — agentic step handlers for crux-script pipelines.
//!
//! Call `register_all(&mut registry)` to install all handlers (including stdlib
//! and optionally BAML), or call each module's `register` function individually.
//!
//! The `judge::*` handlers are the one conditional registration: they appear only
//! when [`TYPESAFE_API_KEY_ENV`] holds a TypeSafe API key, so an unconfigured
//! workspace sees exactly the handler set it saw before `crux-typesafe` existed.

pub mod adapters;
pub mod analysis;
pub mod ci;
pub mod container;
pub mod discover;
pub mod error;
pub mod handlers;
pub mod harness;
pub mod http;
pub mod llm;
pub mod llm_step;
pub mod provider;
pub mod review;
pub mod rx;
pub mod sqlite;
pub mod task;
pub mod triage;

/// Backwards-compatible re-export of shell handlers now owned by `crux-stdlib`.
pub use crux_stdlib::shell;

pub use llm_step::LlmStep;
pub use provider::{LlmProvider, LlmRequest, LlmResponse};

use crux_runtime::prelude::CruxErr;
use crux_script::HandlerRegistry;
use serde_json::Value;
use std::future::Future;
use std::sync::Arc;

/// Register a named agent closure so that `delegate:` pipeline steps can invoke it.
pub fn register_agent<F, Fut>(registry: &mut HandlerRegistry, name: impl Into<String>, f: F)
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
{
    registry.agent_fn(name, f);
}

/// Register all built-in handlers into the given registry.
///
/// This includes stdlib, agentic, and (with `baml` feature) BAML handlers.
pub fn register_all(registry: &mut HandlerRegistry) {
    register_all_with_plugins(registry, Vec::new());
}

/// Register all built-in handlers, including plugin handler descriptions
/// for the planner.
pub fn register_all_with_plugins(registry: &mut HandlerRegistry, plugin_handlers: Vec<String>) {
    // Stdlib handlers
    crux_stdlib::register_all(registry);

    // Agentic handlers
    analysis::register(registry);
    ci::register(registry);
    container::register(registry);
    harness::register(registry);
    http::register(registry);
    review::register(registry);
    rx::register(registry);
    sqlite::register(registry);
    task::register(registry);
    triage::register(registry);
    // LLM handlers. `crux-baml` owns every `llm::*` handler that makes a model
    // call — `llm::invoke`, `llm::invoke_with_fallback`, `llm::stream`, and the
    // structured ones — so provider selection, retries, and output parsing all
    // live behind BAML. The `LlmProvider` port in `provider` stays public for
    // direct Rust callers.
    crux_baml::register_all_with_plugins(registry, plugin_handlers);

    register_judgment_handlers(registry);
}

/// Environment variable holding the TypeSafe System One API key.
///
/// Follows the same `<PROVIDER>_API_KEY` convention as `OPENAI_API_KEY` and
/// `ANTHROPIC_API_KEY`. While it is unset — or set to whitespace — `judge::score`
/// is left unregistered.
pub const TYPESAFE_API_KEY_ENV: &str = "TYPESAFE_API_KEY";

/// The TypeSafe System One judgment endpoint.
const TYPESAFE_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// Register the `judge::*` handlers, but only when a TypeSafe key is configured.
///
/// The handler is gated rather than always registered so that a workspace with no
/// TypeSafe credentials keeps exactly the handler set it had before this crate
/// existed — no new names in `crux handlers`, no new entry in the planner's
/// handler list, and no behaviour change for any existing pipeline. The cost is
/// that a pipeline naming `judge::score` without a key gets an `unknown_handler`
/// diagnostic at validation time, which names the problem, instead of a runtime
/// authentication failure from a step that could never have worked.
///
/// A key that is present but unusable (an empty value, or a client that cannot
/// initialise its TLS backend) is treated as absent rather than registered
/// half-working; the resulting `unknown_handler` diagnostic is the honest report.
fn register_judgment_handlers(registry: &mut HandlerRegistry) {
    let Some(client) = typesafe_judgment_client() else {
        return;
    };
    crux_typesafe::register(registry, client);
}

/// Build a judgment client from the environment, or `None` when unconfigured.
fn typesafe_judgment_client() -> Option<Arc<dyn crux_typesafe::JudgmentClient>> {
    let key = std::env::var(TYPESAFE_API_KEY_ENV).ok()?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let client = crux_typesafe::HttpJudgmentClient::new(TYPESAFE_ENDPOINT, key).ok()?;
    Some(Arc::new(client))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crux_script::{ConfidenceCapability, HandlerRegistry};

    const JUDGE_SCORE: &str = "judge::score";

    /// Serializes the tests that mutate `TYPESAFE_API_KEY`.
    ///
    /// `nextest` runs every test in its own process, so this is belt-and-braces for
    /// `cargo test`, which shares one process across threads.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Take the environment lock, tolerating poisoning so one failing test does not
    /// cascade into the other.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `judge::score` is registered only when a TypeSafe key is present.
    ///
    /// Gating on the key is what keeps an unconfigured workspace byte-identical to
    /// one without this crate: no key means no handler, so every existing pipeline
    /// behaves exactly as before. A pipeline that names the handler anyway gets an
    /// `unknown_handler` diagnostic naming it, rather than a runtime auth failure
    /// from a step that was never configurable.
    #[test]
    fn judge_score_is_registered_only_when_a_typesafe_key_is_set() {
        let _guard = env_guard();
        let previous = std::env::var(TYPESAFE_API_KEY_ENV).ok();

        // SAFETY: `set_var`/`remove_var` are process-global. Every read and write
        // of this variable in this binary is serialized by `env_guard`, and
        // `nextest` additionally process-isolates each test. The previous value is
        // restored at the end either way.
        unsafe { std::env::remove_var(TYPESAFE_API_KEY_ENV) };

        let mut without_key = HandlerRegistry::new();
        register_all(&mut without_key);
        assert!(
            without_key.get_handler(JUDGE_SCORE).is_none(),
            "{JUDGE_SCORE} must stay unregistered without a key so unconfigured \
             pipelines keep their current handler set"
        );

        unsafe { std::env::set_var(TYPESAFE_API_KEY_ENV, "test-key-not-a-real-credential") };
        let mut with_key = HandlerRegistry::new();
        register_all(&mut with_key);
        let metadata = with_key
            .get_metadata(JUDGE_SCORE)
            .expect("judge::score must be registered when TYPESAFE_API_KEY is set");
        assert_eq!(
            metadata.confidence,
            Some(ConfidenceCapability::Always),
            "a registered handler must declare the confidence contract that lets a \
             YAML pipeline route on its confidence"
        );

        unsafe {
            match previous {
                Some(value) => std::env::set_var(TYPESAFE_API_KEY_ENV, value),
                None => std::env::remove_var(TYPESAFE_API_KEY_ENV),
            }
        }
    }

    /// A key that is present but blank is not a key. Registering on it would hand
    /// back the runtime auth failure the gate exists to avoid.
    #[test]
    fn a_blank_key_does_not_register_the_handler() {
        let _guard = env_guard();
        let previous = std::env::var(TYPESAFE_API_KEY_ENV).ok();

        // SAFETY: as above — serialized by `env_guard`, restored at the end.
        unsafe { std::env::set_var(TYPESAFE_API_KEY_ENV, "   ") };

        let mut registry = HandlerRegistry::new();
        register_all(&mut registry);
        assert!(
            registry.get_handler(JUDGE_SCORE).is_none(),
            "a whitespace-only key must not register {JUDGE_SCORE}"
        );

        unsafe {
            match previous {
                Some(value) => std::env::set_var(TYPESAFE_API_KEY_ENV, value),
                None => std::env::remove_var(TYPESAFE_API_KEY_ENV),
            }
        }
    }
}
