/// cruxx-macros: proc macros for the cruxx agentic DSL.
///
/// Provides `#[cruxx::agent]`, `#[cruxx::harness]`, and `#[cruxx::evolve]`
/// which transform async functions and structs into traced, replayable
/// agent functions and harness profile configs.
use proc_macro::TokenStream;

mod agent;
mod evolve;
mod harness;
mod parse;

/// Marks a struct as a harness profile configuration.
///
/// Generates `Default`, `Serialize`/`Deserialize`, and a `to_profile()` method.
#[proc_macro_attribute]
pub fn harness(attr: TokenStream, item: TokenStream) -> TokenStream {
    harness::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as an evolution agent.
///
/// Same as `#[cruxx::agent]` but semantically marks the function as part of
/// the harness evolution loop. Generates an `is_evolution_agent()` method.
#[proc_macro_attribute]
pub fn evolve(attr: TokenStream, item: TokenStream) -> TokenStream {
    evolve::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as a cruxx agent.
///
/// Injects a `CruxCtx` binding called `x`, wraps the return type from
/// `Result<T, CruxErr>` into `Crux<T>`, and generates an `Agent` trait impl.
///
/// # Options
///
/// - `registry = "name"` — bind to a TaskRegistry for checkpointing
/// - `checkpoint_every_step` — checkpoint after every `x.step()` call
/// - `replay = "strict"|"lenient"` — replay mode (default: strict)
#[proc_macro_attribute]
pub fn agent(attr: TokenStream, item: TokenStream) -> TokenStream {
    agent::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
