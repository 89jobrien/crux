/// Recovery strategies for lifecycle hooks.
use std::future::Future;
use std::pin::Pin;

use super::error::CruxErr;

pub enum Recovery<T> {
    /// Re-run the same step.
    Retry,
    /// Re-run with a different closure.
    RetryWith(Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<T, CruxErr>> + Send>> + Send>),
    /// Use this value instead of the step's output.
    Substitute(T),
    /// Run this future as an escalation path.
    Escalate(Pin<Box<dyn Future<Output = Result<T, CruxErr>> + Send>>),
    /// Let the error propagate to the caller.
    Propagate,
    /// Mark the step as skipped and continue.
    Skip,
    /// Ignore the low confidence and continue with the value.
    Continue,
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
