//! Provider-neutral LLM request, response, and completion port types.
//!
//! `LlmRequest` and `LlmResponse` are re-exported from `crux-types` so that
//! downstream crates that only need the wire types can depend on the lighter
//! `crux-types` crate instead of pulling in `crux-agentic`.

pub use crux_types::llm::{LlmRequest, LlmResponse};

use crux_runtime::prelude::CruxErr;

/// Port for LLM completion providers.
///
/// Uses RPITIT (`impl Future`) so no `async_trait` macro is needed.
pub trait LlmProvider: Send + Sync + 'static {
    /// Produces a completion for the supplied provider-neutral request.
    fn complete(
        &self,
        req: LlmRequest,
    ) -> impl std::future::Future<Output = Result<LlmResponse, CruxErr>> + Send;
}
