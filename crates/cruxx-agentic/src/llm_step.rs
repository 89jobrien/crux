use std::sync::Arc;

use cruxx_core::context::Context;
use cruxx_core::prelude::CruxErr;

use crate::provider::{LlmProvider, LlmRequest, LlmResponse};

/// Generic adapter that drives any [`LlmProvider`] through the cruxx [`Context`].
///
/// Wraps the provider in an `Arc` so `LlmStep` is cheap to clone and share.
pub struct LlmStep<P: LlmProvider> {
    provider: Arc<P>,
}

impl<P: LlmProvider> LlmStep<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider: Arc::new(provider),
        }
    }

    /// Invoke the provider through the context's step machinery.
    ///
    /// The step is recorded under `step_name`, enabling replay and tracing.
    pub async fn invoke<C: Context>(
        &self,
        ctx: &mut C,
        step_name: &str,
        req: LlmRequest,
    ) -> Result<LlmResponse, CruxErr> {
        let p = Arc::clone(&self.provider);
        ctx.step(step_name, move || async move { p.complete(req).await })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxx_core::prelude::CruxErr;

    /// In-memory mock that returns a fixed response.
    struct MockLlmProvider {
        response: LlmResponse,
    }

    impl LlmProvider for MockLlmProvider {
        fn complete(
            &self,
            _req: LlmRequest,
        ) -> impl std::future::Future<Output = Result<LlmResponse, CruxErr>> + Send {
            let resp = self.response.clone();
            async move { Ok(resp) }
        }
    }

    #[tokio::test]
    async fn llm_step_returns_provider_response() {
        use cruxx_core::prelude::CruxCtx;

        let mock = MockLlmProvider {
            response: LlmResponse {
                text: "hello world".into(),
                provider: "mock/test".into(),
                metadata: None,
            },
        };
        let step = LlmStep::new(mock);
        let mut ctx = CruxCtx::new("test-agent");
        let req = LlmRequest {
            prompt: "say hello".into(),
            system: None,
            max_tokens: 64,
        };
        let resp = step.invoke(&mut ctx, "test_step", req).await.unwrap();
        assert_eq!(resp.text, "hello world");
        assert_eq!(resp.provider, "mock/test");
    }

    #[tokio::test]
    async fn llm_step_propagates_error() {
        use cruxx_core::prelude::CruxCtx;

        struct FailingProvider;
        impl LlmProvider for FailingProvider {
            async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, CruxErr> {
                Err(CruxErr::step_failed("mock", "intentional failure"))
            }
        }

        let step = LlmStep::new(FailingProvider);
        let mut ctx = CruxCtx::new("test-agent");
        let req = LlmRequest::default();
        let err = step.invoke(&mut ctx, "fail_step", req).await.unwrap_err();
        assert!(err.failed_step().is_some());
    }
}
