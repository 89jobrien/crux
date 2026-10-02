//! The judgment port and its HTTP adapter.
//!
//! [`JudgmentClient`] is the seam: production wires [`HttpJudgmentClient`], tests
//! wire a canned responder. That is what keeps `CONFORMANCE.md`'s rule honest —
//! *"All tests MUST pass without API keys or network access."* Nothing in the
//! handler or calibration path requires a network.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde_json::Value;

use crate::wire::{Answer, SystemOneRequest, SystemOneResponse};

/// Transport for a System One evaluation.
///
/// Implemented by the real HTTP client and by test doubles.
///
/// The boxed future keeps the trait dyn-compatible so callers can hold an
/// `Arc<dyn JudgmentClient>`, which is what lets one registered handler serve
/// both the HTTP backend and a canned one.
pub trait JudgmentClient: Send + Sync {
    /// Evaluate `request`, returning the full response.
    fn evaluate(
        &self,
        request: SystemOneRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SystemOneResponse, ClientError>> + Send + '_>>;
}

/// Failures reaching or reading the judgment backend.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The request could not be built or sent.
    #[error("judgment transport failed: {0}")]
    Transport(String),

    /// The backend answered with a non-success status.
    #[error("judgment backend returned {status}: {body}")]
    Status {
        /// HTTP status code.
        status: u16,
        /// Response body, truncated.
        body: String,
    },

    /// The response body did not match the documented shape.
    #[error("judgment response could not be parsed: {0}")]
    Decode(String),

    /// The requested question id was absent from the response.
    #[error("the judgment response has no answer for question '{question}'")]
    MissingAnswer {
        /// The question id that was requested.
        question: String,
    },
}

impl ClientError {
    /// Whether the failure is worth retrying.
    ///
    /// `429` and `529` are the two statuses the TypeSafe docs name as transient;
    /// everything else is a permanent failure that a retry cannot fix.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Status { status, .. } => *status == 429 || *status == 529,
            Self::Transport(_) => true,
            Self::Decode(_) | Self::MissingAnswer { .. } => false,
        }
    }
}

/// Extract one answer from a response, failing loudly when it is absent.
pub fn take_answer<'a>(
    response: &'a SystemOneResponse,
    question: &str,
) -> Result<&'a Answer, ClientError> {
    response
        .answers
        .get(question)
        .ok_or_else(|| ClientError::MissingAnswer {
            question: question.to_owned(),
        })
}

/// How many times a transient failure is retried, and how long to wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts, including the first.
    pub attempts: u32,
    /// Delay before the second attempt; doubled each time after.
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            base_delay: Duration::from_millis(200),
        }
    }
}

impl RetryPolicy {
    /// Whether another attempt is permitted after `attempt` failures.
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.attempts
    }

    /// Backoff before attempt number `attempt` (1-based).
    ///
    /// Attempt 1 waits `base_delay`, and each later attempt doubles it. The
    /// exponent is capped so a pathological attempt count saturates rather than
    /// overflowing into a panic or a wrap back to zero.
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(16);
        self.base_delay.saturating_mul(1_u32 << exponent)
    }
}

/// A `JudgmentClient` backed by `reqwest`.
#[cfg(feature = "http")]
#[derive(Debug, Clone)]
pub struct HttpJudgmentClient {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    api_key: String,
    retry: RetryPolicy,
    max_response_bytes: usize,
}

#[cfg(feature = "http")]
impl HttpJudgmentClient {
    /// Largest response body accepted, to bound a malformed or hostile reply.
    pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 1 << 20;

    /// Build a client against `endpoint`, authenticating with `api_key`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] when the bounded client cannot be built
    /// or the key is empty.
    pub fn new(endpoint: reqwest::Url, api_key: impl Into<String>) -> Result<Self, ClientError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(ClientError::Transport(
                "the judgment backend API key is empty".to_owned(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        Ok(Self {
            client,
            endpoint,
            api_key,
            retry: RetryPolicy::default(),
            max_response_bytes: Self::DEFAULT_MAX_RESPONSE_BYTES,
        })
    }

    /// Override the retry policy.
    #[must_use]
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Override the response size ceiling.
    #[must_use]
    pub fn with_max_response_bytes(mut self, max_response_bytes: usize) -> Self {
        self.max_response_bytes = max_response_bytes;
        self
    }

    async fn attempt(&self, request: &SystemOneRequest) -> Result<SystemOneResponse, ClientError> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.api_key)
            .json(request)
            .send()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;

        let status = response.status().as_u16();
        let body = read_bounded(response, self.max_response_bytes).await?;

        if !(200..300).contains(&status) {
            return Err(ClientError::Status {
                status,
                body: truncate(&String::from_utf8_lossy(&body), 512),
            });
        }

        serde_json::from_slice(&body).map_err(|error| ClientError::Decode(error.to_string()))
    }
}

#[cfg(feature = "http")]
async fn read_bounded(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, ClientError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| ClientError::Transport(error.to_string()))?;
    if bytes.len() > max_bytes {
        return Err(ClientError::Transport(format!(
            "judgment response of {} bytes exceeds the {max_bytes} byte ceiling",
            bytes.len()
        )));
    }
    Ok(bytes.to_vec())
}

#[cfg(feature = "http")]
fn truncate(body: &str, max: usize) -> String {
    if body.len() <= max {
        return body.to_owned();
    }
    let mut end = max;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    body[..end].to_owned()
}

#[cfg(feature = "http")]
impl JudgmentClient for HttpJudgmentClient {
    fn evaluate(
        &self,
        request: SystemOneRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SystemOneResponse, ClientError>> + Send + '_>> {
        Box::pin(async move {
            let mut attempt = 1;
            loop {
                let error = match self.attempt(&request).await {
                    Ok(response) => return Ok(response),
                    Err(error) => error,
                };
                if !error.is_transient() || !self.retry.should_retry(attempt) {
                    return Err(error);
                }
                tokio::time::sleep(self.retry.delay_for(attempt)).await;
                attempt += 1;
            }
        })
    }
}

/// A canned [`JudgmentClient`] for tests and dry runs.
///
/// Returns queued responses in order, repeating the last one once exhausted, so a
/// pipeline that retries sees stable answers rather than a panic.
#[derive(Debug, Clone, Default)]
pub struct CannedJudgmentClient {
    responses: std::sync::Arc<std::sync::Mutex<Vec<SystemOneResponse>>>,
    default_response: Option<SystemOneResponse>,
}

impl CannedJudgmentClient {
    /// An empty client that fails every call.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Queue a response.
    #[must_use]
    pub fn push(self, response: SystemOneResponse) -> Self {
        self.responses
            .lock()
            .expect("canned client mutex poisoned")
            .push(response);
        self
    }

    /// Set the response returned once the queue is drained.
    #[must_use]
    pub fn or(mut self, response: SystemOneResponse) -> Self {
        self.default_response = Some(response);
        self
    }
}

impl JudgmentClient for CannedJudgmentClient {
    fn evaluate(
        &self,
        _request: SystemOneRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SystemOneResponse, ClientError>> + Send + '_>> {
        Box::pin(async move {
            let mut queue = self.responses.lock().expect("canned client mutex poisoned");
            if queue.is_empty() {
                return self.default_response.clone().ok_or_else(|| {
                    ClientError::Transport("canned client has no response".to_owned())
                });
            }
            if queue.len() == 1 {
                return Ok(queue[0].clone());
            }
            Ok(queue.remove(0))
        })
    }
}

/// Build a `SystemOneRequest` carrying a single named question.
pub fn single_question_request(
    state: Value,
    question_id: &str,
    question: Value,
) -> SystemOneRequest {
    SystemOneRequest {
        state,
        model: crate::wire::DEFAULT_MODEL.to_owned(),
        questions: std::collections::BTreeMap::from([(question_id.to_owned(), question)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Answer, ChoiceAnswer, ScoreAnswer};
    use serde_json::json;

    fn score_response() -> SystemOneResponse {
        SystemOneResponse {
            model: "jev-test".to_owned(),
            answers: [(
                "severity".to_owned(),
                Answer::Score(ScoreAnswer {
                    score: 1.0,
                    legend: [
                        ("0".to_owned(), json!("low")),
                        ("1".to_owned(), json!("high")),
                    ]
                    .into_iter()
                    .collect(),
                    probabilities: [("0".to_owned(), 0.5), ("1".to_owned(), 0.5)]
                        .into_iter()
                        .collect(),
                    confidence: 0.5,
                }),
            )]
            .into_iter()
            .collect(),
            usage: None,
        }
    }

    #[test]
    fn a_canned_client_replays_its_only_response() {
        let client = CannedJudgmentClient::empty().push(score_response());
        let request = single_question_request(json!("x"), "severity", json!({}));
        let first = futures_lite_block_on(client.evaluate(request.clone()));
        let second = futures_lite_block_on(client.evaluate(request));
        assert!(first.is_ok());
        assert!(second.is_ok());
    }

    #[test]
    fn a_canned_client_advances_through_its_queue() {
        let mut second = score_response();
        if let Some(Answer::Score(answer)) = second.answers.get_mut("severity") {
            answer.score = 2.0;
        }
        let client = CannedJudgmentClient::empty()
            .push(score_response())
            .push(second);
        let request = single_question_request(json!("x"), "severity", json!({}));

        let first = futures_lite_block_on(client.evaluate(request.clone()));
        let second_response = futures_lite_block_on(client.evaluate(request));
        assert!(matches!(
            first.unwrap().answers.get("severity"),
            Some(Answer::Score(answer)) if (answer.score - 1.0).abs() < 1e-9
        ));
        assert!(matches!(
            second_response.unwrap().answers.get("severity"),
            Some(Answer::Score(answer)) if (answer.score - 2.0).abs() < 1e-9
        ));
    }

    #[test]
    fn an_empty_canned_client_fails_loudly() {
        let client = CannedJudgmentClient::empty();
        let request = single_question_request(json!("x"), "severity", json!({}));
        let error = futures_lite_block_on(client.evaluate(request)).unwrap_err();
        assert!(matches!(error, ClientError::Transport(_)));
    }

    #[test]
    fn take_answer_reports_a_missing_question() {
        let response = score_response();
        let error = take_answer(&response, "nope").unwrap_err();
        assert!(matches!(error, ClientError::MissingAnswer { .. }));
        assert!(take_answer(&response, "severity").is_ok());
    }

    /// Only 429 and 529 are retryable, per the TypeSafe docs. A 401 must fail
    /// immediately — retrying a bad key just burns the attempt budget.
    #[test]
    fn only_the_documented_transient_statuses_retry() {
        for status in [429_u16, 529] {
            assert!(
                ClientError::Status {
                    status,
                    body: String::new()
                }
                .is_transient(),
                "{status} must be transient"
            );
        }
        for status in [400_u16, 401, 403, 422, 500] {
            assert!(
                !ClientError::Status {
                    status,
                    body: String::new()
                }
                .is_transient(),
                "{status} must not be transient"
            );
        }
    }

    #[test]
    fn a_decode_failure_is_not_retried() {
        assert!(!ClientError::Decode("bad".to_owned()).is_transient());
        assert!(
            !ClientError::MissingAnswer {
                question: "x".to_owned()
            }
            .is_transient()
        );
    }

    #[test]
    fn the_retry_budget_is_finite() {
        let retry = RetryPolicy {
            attempts: 3,
            base_delay: Duration::from_millis(1),
        };
        assert!(retry.should_retry(1));
        assert!(retry.should_retry(2));
        assert!(!retry.should_retry(3));
    }

    #[test]
    fn backoff_grows_and_stays_finite() {
        let retry = RetryPolicy {
            attempts: 8,
            base_delay: Duration::from_millis(10),
        };
        assert_eq!(retry.delay_for(1), Duration::from_millis(10));
        assert_eq!(retry.delay_for(2), Duration::from_millis(20));
        assert_eq!(retry.delay_for(3), Duration::from_millis(40));
        // A pathological attempt count must not overflow into a panic or a wrap.
        assert!(retry.delay_for(u32::MAX).as_secs() < u64::from(u32::MAX));
    }

    /// A choice answer is not a score; asking for one must not silently succeed.
    #[test]
    fn a_choice_answer_is_visible_as_its_own_variant() {
        let response = SystemOneResponse {
            model: "jev-test".to_owned(),
            answers: [(
                "route".to_owned(),
                Answer::Choice(ChoiceAnswer {
                    choice: "billing".to_owned(),
                    probabilities: [("billing".to_owned(), 1.0)].into_iter().collect(),
                    confidence: 1.0,
                }),
            )]
            .into_iter()
            .collect(),
            usage: None,
        };
        let answer = take_answer(&response, "route").unwrap();
        assert!(matches!(answer, Answer::Choice(_)));
        assert_eq!(answer.concentration(), Some(1.0));
    }

    fn futures_lite_block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
            .block_on(future)
    }
}
