/// The Agent trait — a delegatable unit of agentic work.
///
/// Agents have typed inputs and outputs, a name, and optional lifecycle hooks.
/// You rarely implement this directly — the `#[crux::agent]` macro generates
/// an impl from a free function.
// TODO(#93): token-shape step priority — infer priority from naming convention
//   (ALL_CAPS -> Max, TitleCase -> High, snake_case -> Lowest) as lightweight
//   scheduling hints (cf. slash)
use serde::{Serialize, de::DeserializeOwned};

use crate::types::budget::Budget;
use crate::types::error::CruxErr;
use crate::types::recovery::Recovery;

/// Port: defines what an agent must provide.
///
/// Lifecycle hooks have sensible defaults so simple agents only need `name()` and `run()`.
pub trait Agent: Send + Sync + 'static {
    type Input: Serialize + DeserializeOwned + Send;
    type Output: Serialize + DeserializeOwned + Send;

    fn name() -> &'static str;

    /// Execute the agent's logic.
    ///
    /// The CruxCtx is injected by the macro as `x`. Manual implementors
    /// receive it as the first argument. The Context trait provides the
    /// abstraction boundary for testing.
    fn run(
        ctx: &mut crate::ctx::CruxCtx,
        input: Self::Input,
    ) -> impl std::future::Future<Output = Result<Self::Output, CruxErr>> + Send;

    fn budget() -> Budget {
        Budget::default()
    }

    fn on_low_confidence(_score: f32) -> Recovery<Self::Output> {
        Recovery::Continue
    }

    fn on_step_failure(_err: &CruxErr) -> Recovery<Self::Output> {
        Recovery::Propagate
    }
}
