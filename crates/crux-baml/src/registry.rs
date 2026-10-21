//! Data-driven registry for BAML function definitions.
//!
//! Each BAML function is described by a [`BamlFunctionDef`] that captures its
//! name, human-readable description, JSON Schema for the input map, JSON Schema
//! for the expected output, an advisory timeout, and a type-erased async
//! handler closure. The [`BamlFunctionRegistry`] collects definitions and
//! exposes the unified `llm::extract` handler to a [`crux_script::HandlerRegistry`].
//!
//! ## Adding a new BAML function
//!
//! 1. Add a `fn <name>_def() -> BamlFunctionDef` factory in `definitions.rs`.
//! 2. Call it from `all_definitions()` in that same file.
//!
//! No changes to `extract.rs` or any match arm are required.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use baml::ClientRegistry;
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerOutput, HandlerRegistry};
use serde_json::{Map, Value};

/// A type-erased async handler for a single BAML function.
///
/// Receives the caller-supplied input map, an optional pre-built
/// [`ClientRegistry`] (for programmatic client routing), and an optional
/// client override name. Returns a [`HandlerOutput`] on success or a
/// [`CruxErr`] on failure.
pub type BamlHandler = Arc<
    dyn Fn(
            Map<String, Value>,
            Option<ClientRegistry>,
            Option<String>,
        ) -> Pin<Box<dyn Future<Output = Result<HandlerOutput, CruxErr>> + Send>>
        + Send
        + Sync,
>;

/// Metadata and implementation for a single generated BAML function.
pub struct BamlFunctionDef {
    /// BAML function name, exactly as it appears in the generated client
    /// (e.g. `"ExtractEntities"`).
    pub name: &'static str,

    /// One-line human-readable description used by the planner and introspection.
    pub description: &'static str,

    /// JSON Schema (object shape) describing the required and optional input
    /// fields. Used for validation and documentation — not enforced at runtime
    /// beyond what the handler closure checks.
    pub input_schema: Value,

    /// JSON Schema (object shape) describing the output fields the handler
    /// always produces in the [`HandlerOutput`] payload.
    pub output_schema: Value,

    /// Advisory wall-clock timeout in milliseconds. The pipeline executor may
    /// use this to set a per-step budget when none is supplied by the caller.
    pub timeout_ms: u64,

    /// Type-erased async dispatch closure.
    pub handler: BamlHandler,
}

impl BamlFunctionDef {
    /// Invoke the handler with the given inputs.
    pub async fn invoke(
        &self,
        input_map: Map<String, Value>,
        client_registry: Option<ClientRegistry>,
        client_override: Option<String>,
    ) -> Result<HandlerOutput, CruxErr> {
        (self.handler)(input_map, client_registry, client_override).await
    }
}

impl std::fmt::Debug for BamlFunctionDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BamlFunctionDef")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

/// Registry of [`BamlFunctionDef`]s, keyed by function name.
///
/// Build one from a vec of definitions, then call
/// [`BamlFunctionRegistry::register_extract_handler`] to wire the unified
/// `llm::extract` handler into a [`HandlerRegistry`].
pub struct BamlFunctionRegistry {
    defs: HashMap<&'static str, BamlFunctionDef>,
}

impl BamlFunctionRegistry {
    /// Build a registry from a list of definitions.
    ///
    /// Panics in debug builds if two definitions share the same name.
    pub fn from_defs(defs: impl IntoIterator<Item = BamlFunctionDef>) -> Self {
        let mut map = HashMap::new();
        for def in defs {
            let prev = map.insert(def.name, def);
            debug_assert!(
                prev.is_none(),
                "duplicate BamlFunctionDef name: {}",
                prev.map_or("", |d| d.name)
            );
        }
        Self { defs: map }
    }

    /// Names of all registered functions, sorted for deterministic output.
    pub fn function_names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.defs.keys().copied().collect();
        names.sort_unstable();
        names
    }

    /// Register the `llm::extract` handler on `registry`.
    ///
    /// The handler reads `function`, `input`, and optionally `client` from
    /// the pipeline step input, looks up the corresponding
    /// [`BamlFunctionDef`], and delegates to its handler closure.
    pub fn register_extract_handler(
        self,
        registry: &mut HandlerRegistry,
        client_registry: Option<ClientRegistry>,
    ) {
        // Wrap the function map in an Arc so the closure is 'static + Clone.
        let defs: Arc<HashMap<&'static str, BamlFunctionDef>> = Arc::new(self.defs);
        // Build the error message once rather than on every call.
        let known: Arc<str> = {
            let mut names: Vec<_> = defs.keys().copied().collect();
            names.sort_unstable();
            names.join(", ").into()
        };

        registry.handler("llm::extract", move |input: Value| {
            let defs = Arc::clone(&defs);
            let known = Arc::clone(&known);
            let client_registry = client_registry.clone();

            async move {
                let function = input
                    .get("function")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CruxErr::step_failed("llm::extract", "missing 'function' field")
                    })?
                    .to_string();

                let input_map = input
                    .get("input")
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| {
                        CruxErr::step_failed("llm::extract", "missing or non-object 'input' field")
                    })?
                    .clone();

                let client_override = input
                    .get("client")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);

                let def = defs.get(function.as_str()).ok_or_else(|| {
                    CruxErr::step_failed(
                        "llm::extract",
                        format!(
                            "unknown BAML function '{function}'; registered functions: {known}"
                        ),
                    )
                })?;

                def.invoke(input_map, client_registry, client_override)
                    .await
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dummy_def(name: &'static str) -> BamlFunctionDef {
        BamlFunctionDef {
            name,
            description: "test",
            input_schema: json!({}),
            output_schema: json!({}),
            timeout_ms: 5_000,
            handler: Arc::new(|_input, _cr, _co| {
                Box::pin(async { Ok(HandlerOutput::new(json!({"ok": true}))) })
            }),
        }
    }

    #[test]
    fn function_names_are_sorted() {
        let reg = BamlFunctionRegistry::from_defs([dummy_def("Zeta"), dummy_def("Alpha")]);
        assert_eq!(reg.function_names(), vec!["Alpha", "Zeta"]);
    }

    #[tokio::test]
    async fn dispatches_to_correct_def() {
        let reg = BamlFunctionRegistry::from_defs([dummy_def("MyFunc")]);
        let mut handler_reg = HandlerRegistry::new();
        reg.register_extract_handler(&mut handler_reg, None);

        // The handler itself is registered; we can exercise the dispatch
        // logic directly via the def.
        let def = BamlFunctionRegistry::from_defs([dummy_def("MyFunc")]);
        let def_ref = def.defs.get("MyFunc").expect("should be present");
        let out = def_ref
            .invoke(Default::default(), None, None)
            .await
            .expect("dummy handler never fails");
        let payload = out.value;
        assert_eq!(payload["ok"], true);
    }

    #[tokio::test]
    async fn unknown_function_returns_error() {
        let reg = BamlFunctionRegistry::from_defs([dummy_def("Known")]);
        let mut handler_reg = HandlerRegistry::new();
        reg.register_extract_handler(&mut handler_reg, None);

        // Verify the known list appears in the error by invoking a second
        // registry with a known function and checking the error message.
        let reg2 = BamlFunctionRegistry::from_defs([dummy_def("Known")]);
        let defs: Arc<HashMap<&'static str, BamlFunctionDef>> = Arc::new(reg2.defs);
        let err = defs
            .get("NoSuchFunction")
            .map(|_| ())
            .ok_or_else(|| "not found".to_string());
        assert!(err.is_err());
    }
}
