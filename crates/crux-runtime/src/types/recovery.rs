//! Runtime recovery actions, including closure- and future-bearing variants.

/// Recovery strategies for lifecycle hooks.
use std::future::Future;
use std::pin::Pin;

use super::error::CruxErr;

/// A boxed future returning `Result<T, CruxErr>`.
pub type BoxFut<T> = Pin<Box<dyn Future<Output = Result<T, CruxErr>> + Send>>;

pub enum Recovery<T> {
    /// Re-run the same step.
    Retry,
    /// Re-run with a different closure.
    RetryWith(Box<dyn FnOnce() -> BoxFut<T> + Send>),
    /// Use this value instead of the step's output.
    Substitute(T),
    /// Run this future as an escalation path.
    Escalate(BoxFut<T>),
    /// Let the error propagate to the caller.
    Propagate,
    /// Mark the step as skipped and continue.
    Skip,
    /// Ignore the low confidence and continue with the value.
    Continue,
}

/// Ordered, inspectable fallback strategies for graph-like recovery flows.
#[derive(Debug, Default)]
pub struct RecoveryChain<T> {
    strategies: std::collections::VecDeque<Recovery<T>>,
}

impl<T> RecoveryChain<T> {
    pub fn new() -> Self {
        Self {
            strategies: std::collections::VecDeque::new(),
        }
    }

    pub fn then(mut self, strategy: Recovery<T>) -> Self {
        self.strategies.push_back(strategy);
        self
    }
}

impl<T> Iterator for RecoveryChain<T> {
    type Item = Recovery<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.strategies.pop_front()
    }
}

impl<T> std::fmt::Debug for Recovery<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retry => write!(f, "Recovery::Retry"),
            Self::RetryWith(_) => write!(f, "Recovery::RetryWith(...)"),
            Self::Substitute(_) => write!(f, "Recovery::Substitute(...)"),
            Self::Escalate(_) => write!(f, "Recovery::Escalate(...)"),
            Self::Propagate => write!(f, "Recovery::Propagate"),
            Self::Skip => write!(f, "Recovery::Skip"),
            Self::Continue => write!(f, "Recovery::Continue"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_chain_preserves_declared_fallback_order() {
        let mut chain = RecoveryChain::new()
            .then(Recovery::Retry)
            .then(Recovery::Substitute(42));

        assert!(matches!(chain.next(), Some(Recovery::Retry)));
        assert!(matches!(chain.next(), Some(Recovery::Substitute(42))));
        assert!(chain.next().is_none());
    }
}
