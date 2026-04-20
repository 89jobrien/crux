/// cruxx-macros: proc macros for the cruxx agentic DSL.
///
/// Provides `#[cruxx::agent]` which transforms async functions into
/// traced, replayable agent functions.
use proc_macro::TokenStream;

mod agent;
mod parse;

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
