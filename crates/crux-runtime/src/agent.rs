//! Typed agent port and default lifecycle recovery behavior.

/// The Agent trait — a delegatable unit of agentic work.
///
/// Agents have typed inputs and outputs, a name, and optional lifecycle hooks.
/// You rarely implement this directly — the `#[crux::agent]` macro generates
/// an impl from a free function.
use serde::{Serialize, de::DeserializeOwned};

use crate::types::budget::Budget;
use crate::types::error::CruxErr;
use crate::types::recovery::Recovery;

/// Infer a lightweight scheduling hint from a step name's token shape.
///
/// `ALL_CAPS` names are critical, `TitleCase` names are high priority,
/// `snake_case` names are deferred, and unrecognized shapes remain medium.
pub fn infer_priority(name: &str) -> slashcrux::Priority {
    let letters: Vec<char> = name
        .chars()
        .filter(|character| character.is_alphabetic())
        .collect();
    if !letters.is_empty() && letters.iter().all(|character| character.is_uppercase()) {
        return slashcrux::Priority::Critical;
    }

    let mut characters = name.chars();
    if let Some(first) = characters.next()
        && first.is_uppercase()
        && characters
            .filter(|character| character.is_alphabetic())
            .all(|character| character.is_lowercase())
        && !name.contains('_')
    {
        return slashcrux::Priority::High;
    }

    if name.contains('_')
        && name.chars().all(|character| {
            character.is_lowercase() || character.is_ascii_digit() || character == '_'
        })
    {
        return slashcrux::Priority::Deferred;
    }

    slashcrux::Priority::Medium
}

/// Port: defines what an agent must provide.
///
/// Lifecycle hooks have sensible defaults so simple agents only need `name()` and `run()`.
pub trait Agent: Send + Sync + 'static {
    type Input: Serialize + DeserializeOwned + Send;
    type Output: Serialize + DeserializeOwned + Send;

    /// Returns the stable name recorded for this agent's trace.
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

    /// Returns the execution budget applied to a run of this agent.
    fn budget() -> Budget {
        Budget::default()
    }

    /// Chooses recovery behavior when a step reports low confidence.
    fn on_low_confidence(_score: f32) -> Recovery<Self::Output> {
        Recovery::Continue
    }

    /// Chooses recovery behavior after a step fails.
    fn on_step_failure(_err: &CruxErr) -> Recovery<Self::Output> {
        Recovery::Propagate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slashcrux::Priority;

    #[test]
    fn token_shape_infers_scheduling_priority() {
        assert_eq!(infer_priority("RELEASE"), Priority::Critical);
        assert_eq!(infer_priority("Review"), Priority::High);
        assert_eq!(infer_priority("review_changes"), Priority::Deferred);
        assert_eq!(infer_priority("review-changes"), Priority::Medium);
    }
}
