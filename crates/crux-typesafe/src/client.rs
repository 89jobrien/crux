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
///
/// `#[non_exhaustive]` so a future failure mode — a rate-limit header, a partial
/// answer — can be added without breaking every downstream `match`. Callers
/// outside this crate must already carry a wildcard arm.
#[non_exhaustive]
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
    /// # What retries
    ///
    /// **Every `5xx`, plus `429`.** A server error is the backend failing rather
    /// than the request: both official TypeSafe SDKs retry the whole `5xx` range
    /// with exponential backoff, and `429` is the rate limit, which clears on its
    /// own. Treating a `500` as permanent turned a transient blip into a hard
    /// pipeline failure.
    ///
    /// # What does not retry
    ///
    /// **Every other `4xx`.** A `401` means the key is wrong and a `422` means the
    /// question is malformed; sending the identical request again produces the
    /// identical rejection, so retrying only burns the attempt budget and delays
    /// the real error. `429` is the one `4xx` that clears, which is why it is
    /// called out separately rather than swept in with the server errors.
    ///
    /// A [`Self::Decode`] or [`Self::MissingAnswer`] failure is never transient —
    /// the bytes on the wire already arrived and re-reading them cannot change the
    /// answer. A [`Self::Transport`] failure is, since a connection that failed to
    /// establish is the most transient thing there is.
    pub fn is_transient(&self) -> bool {
        match self {
            // `529` is a non-standard "site is overloaded" status; it lands in the
            // 5xx range and is named explicitly in the TypeSafe error table.
            Self::Status { status, .. } => *status == 429 || (500..600).contains(status),
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
    /// `endpoint` is taken as a `&str` and parsed here rather than as a
    /// [`reqwest::Url`]. A public signature naming a transitive dependency's
    /// type forces every downstream user to add `reqwest` to their own
    /// `Cargo.toml` — and pin a matching version — purely to name the argument.
    /// A string keeps this crate's HTTP client an implementation detail.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] when `endpoint` is not a valid URL,
    /// when the bounded client cannot be built, or when `api_key` is empty or
    /// only whitespace.
    pub fn new(endpoint: &str, api_key: impl Into<String>) -> Result<Self, ClientError> {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|error| {
            ClientError::Transport(format!(
                "the judgment backend endpoint '{endpoint}' is not a valid URL: {error}"
            ))
        })?;
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
/// # Exhaustion
///
/// Queued responses are returned **in order, and the last one then repeats
/// forever.** The queue never runs dry: once only the final entry is left, every
/// further call returns it again.
///
/// That is deliberate. The client stands in for a backend that a pipeline may
/// call more than once — a retry, a second route, a re-run of one step — and a
/// drain-to-error rule would turn those extra calls into failures. Repeating the
/// last answer is what lets a retrying pipeline get a stable judgment.
///
/// The consequence is that **there is no separate fallback response**: once
/// anything is pushed, the last thing pushed is what every later call sees. Use
/// [`Self::push`] once to say "always answer this".
#[derive(Debug, Clone, Default)]
pub struct CannedJudgmentClient {
    responses: std::sync::Arc<std::sync::Mutex<Vec<SystemOneResponse>>>,
}

impl CannedJudgmentClient {
    /// A client with nothing queued, which fails every call.
    ///
    /// Useful for registering a handler in a test that never invokes it.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Queue a response, or replace the answer a client already repeats.
    ///
    /// Pushing onto an empty client makes that response the permanent answer; see
    /// [`CannedJudgmentClient`]'s exhaustion rules.
    #[must_use]
    pub fn push(self, response: SystemOneResponse) -> Self {
        self.responses
            .lock()
            .expect("canned client mutex poisoned")
            .push(response);
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
            let Some(first) = queue.first() else {
                return Err(ClientError::Transport(
                    "canned client has no response".to_owned(),
                ));
            };
            // Popping every entry but the last is what makes the last one repeat
            // without a separate "exhausted" state to keep in sync.
            if queue.len() > 1 {
                return Ok(queue.remove(0));
            }
            Ok(first.clone())
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

    /// A `5xx` is the backend failing, not us, and both official TypeSafe SDKs
    /// retry every server error with exponential backoff. A `4xx` is *us*
    /// failing: retrying a bad key or a malformed question cannot succeed, so it
    /// must fail fast instead of burning the attempt budget.
    #[test]
    fn server_errors_retry_while_client_errors_fail_fast() {
        for status in [500_u16, 502, 503, 504, 529] {
            assert!(
                ClientError::Status {
                    status,
                    body: String::new()
                }
                .is_transient(),
                "{status} is a server error and must be transient"
            );
        }

        // 429 is a 4xx yet is explicitly retryable: the rate limit will clear.
        assert!(
            ClientError::Status {
                status: 429,
                body: String::new()
            }
            .is_transient(),
            "429 is the documented rate limit and must be transient"
        );

        for status in [400_u16, 401, 403, 404, 422] {
            assert!(
                !ClientError::Status {
                    status,
                    body: String::new()
                }
                .is_transient(),
                "{status} is a client error; a retry cannot fix it"
            );
        }
    }

    /// The server-error range is half-open `[500, 600)`. An unassigned `6xx` is
    /// not a server error, and treating it as one would retry a status that means
    /// nothing rather than one that might clear.
    #[test]
    fn the_server_error_range_is_five_hundred_to_five_ninety_nine() {
        for status in [199_u16, 300, 400, 499, 600, 700] {
            assert!(
                !ClientError::Status {
                    status,
                    body: String::new()
                }
                .is_transient(),
                "{status} is outside the server-error range"
            );
        }
        for status in [500_u16, 550, 599] {
            assert!(
                ClientError::Status {
                    status,
                    body: String::new()
                }
                .is_transient(),
                "{status} is inside the server-error range"
            );
        }
    }

    /// The queue **repeats its last entry** rather than draining, and that is the
    /// contract — there is no separate fallback.
    ///
    /// `or` used to be documented as "the response returned once the queue is
    /// drained", which cannot happen: the repeat rule means the last pushed
    /// response answers forever, so a fallback configured alongside any `push`
    /// was unreachable. The repeat rule is the load-bearing one — a pipeline that
    /// retries must get the same judgment back, not an error — so it was kept and
    /// the unreachable `or` was removed rather than the rule that works.
    #[test]
    fn the_last_pushed_response_repeats_forever_and_there_is_no_fallback() {
        let mut last = score_response();
        if let Some(Answer::Score(answer)) = last.answers.get_mut("severity") {
            answer.score = 2.0;
        }
        let client = CannedJudgmentClient::empty()
            .push(score_response())
            .push(last);

        let request = single_question_request(json!("x"), "severity", json!({}));

        // Four calls, two responses queued. Call 1 drains the first entry; every
        // later call repeats the last one instead of running off the end.
        for (call, expected) in [(1, 1.0), (2, 2.0), (3, 2.0), (4, 2.0)] {
            let response = futures_lite_block_on(client.evaluate(request.clone()))
                .unwrap_or_else(|error| panic!("call {call} must succeed, got {error}"));
            assert!(
                matches!(
                    response.answers.get("severity"),
                    Some(Answer::Score(answer)) if (answer.score - expected).abs() < 1e-9
                ),
                "call {call} must answer {expected}; a repeat, not a drain"
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
