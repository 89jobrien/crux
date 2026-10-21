//! Integration coverage for the `http` feature: the real [`HttpJudgmentClient`]
//! driven against a loopback mock of the TypeSafe System One endpoint.
//!
//! Before this file the entire `http` adapter was uncovered. That mattered most
//! for the two error mappings most likely to be wrong — [`ClientError::Status`]
//! and [`ClientError::Decode`] — which the unit tests could only hand-build as
//! literals and never exercise through the producing code path.
//!
//! Every server here binds `127.0.0.1:0`, so the suite is hermetic: no external
//! network, no DNS, and no API key. The bearer token is the literal `test-key`.
//!
//! [`HttpJudgmentClient`]: crux_typesafe::HttpJudgmentClient
//! [`ClientError::Status`]: crux_typesafe::ClientError::Status
//! [`ClientError::Decode`]: crux_typesafe::ClientError::Decode
#![cfg(feature = "http")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crux_typesafe::client::{
    ClientError, HttpJudgmentClient, JudgmentClient, RetryPolicy, single_question_request,
    take_answer,
};
use crux_typesafe::wire::{Answer, ScoreQuestion, SystemOneRequest};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

// -- mock backend --

/// One request exactly as the mock backend saw it on the wire.
#[derive(Clone, Debug)]
struct Recorded {
    /// e.g. `POST /v1/systemone HTTP/1.1`.
    request_line: String,
    /// Header names lowercased, so a lookup is not case-sensitive.
    headers: Vec<(String, String)>,
    body: String,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("the client must send valid JSON")
    }
}

/// A loopback stand-in for `POST /v1/systemone`.
struct MockBackend {
    url: String,
    seen: Arc<Mutex<Vec<Recorded>>>,
}

impl MockBackend {
    /// Serve `script` in order. Once the script runs out the last entry repeats,
    /// so a test that does not care about retries can script a single response.
    async fn start(script: &[(u16, String)]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback must be bindable");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));

        let script: Arc<Vec<(u16, String)>> = Arc::new(script.to_vec());
        let served = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));

        let (served_task, seen_task) = (Arc::clone(&served), Arc::clone(&seen));
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (script, served, seen) = (
                    Arc::clone(&script),
                    Arc::clone(&served_task),
                    Arc::clone(&seen_task),
                );
                tokio::spawn(async move {
                    serve_connection(stream, script, served, seen).await;
                });
            }
        });

        Self { url, seen }
    }

    fn requests(&self) -> Vec<Recorded> {
        self.seen.lock().expect("mock mutex").clone()
    }

    fn only_request(&self) -> Recorded {
        let mut requests = self.requests();
        assert_eq!(
            requests.len(),
            1,
            "expected exactly one request, saw {}",
            requests.len()
        );
        requests.remove(0)
    }
}

/// Answer requests on one connection until the client hangs up.
///
/// `reqwest` pools connections, so a single socket can carry several requests
/// and the loop has to keep reading rather than assume one request per accept.
async fn serve_connection(
    stream: TcpStream,
    script: Arc<Vec<(u16, String)>>,
    served: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<Recorded>>>,
) {
    let mut reader = BufReader::new(stream);
    loop {
        let Some(recorded) = read_request(&mut reader).await else {
            return;
        };
        let index = served.fetch_add(1, Ordering::SeqCst);
        let (status, body) = script
            .get(index)
            .or_else(|| script.last())
            .cloned()
            .unwrap_or_else(|| (200, "{}".to_owned()));
        seen.lock().expect("mock mutex").push(recorded);

        let response = format!(
            "HTTP/1.1 {status} {reason}\r\n\
             content-type: application/json\r\n\
             content-length: {length}\r\n\
             \r\n\
             {body}",
            reason = reason_phrase(status),
            length = body.len(),
        );
        let socket = reader.get_mut();
        if socket.write_all(response.as_bytes()).await.is_err() {
            return;
        }
        let _ = socket.flush().await;
    }
}

/// Read one HTTP request, or `None` once the peer closes the connection.
async fn read_request(reader: &mut BufReader<TcpStream>) -> Option<Recorded> {
    let mut request_line = String::new();
    if read_line(reader, &mut request_line).await == 0 {
        return None;
    }

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if read_line(reader, &mut line).await == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }

    let mut body = vec![0_u8; content_length];
    if content_length > 0 && reader.read_exact(&mut body).await.is_err() {
        return None;
    }

    Some(Recorded {
        request_line: request_line.trim_end().to_owned(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// `read_line` that treats a closed socket as end-of-input rather than panicking.
async fn read_line(reader: &mut BufReader<TcpStream>, into: &mut String) -> usize {
    reader.read_line(into).await.unwrap_or(0)
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

// -- fixtures --

/// A `SystemOneResponse` in the documented shape.
fn system_one_body() -> String {
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "severity": {
                "type": "score",
                "score": 1.43,
                "legend": { "0": "Cosmetic", "1": "Workaround", "2": "Blocking" },
                "probabilities": { "0": 0.0, "1": 0.57, "2": 0.43 },
                "confidence": 0.35
            }
        },
        "usage": { "input_tokens": 120, "output_tokens": 40 }
    })
    .to_string()
}

/// A client with a retry budget short enough not to slow the suite down.
fn client_for(url: &str) -> HttpJudgmentClient {
    HttpJudgmentClient::new(url, "test-key")
        .expect("a loopback URL and a non-empty key are both valid")
        .with_retry(RetryPolicy {
            attempts: 3,
            base_delay: Duration::from_millis(1),
        })
}

/// The request a `judge::score` step would send.
fn request() -> SystemOneRequest {
    let question = ScoreQuestion::new("How severe?", vec!["low".into(), "high".into()])
        .expect("two levels is valid");
    single_question_request(
        json!("The export button crashes the settings page."),
        "severity",
        serde_json::to_value(&question).expect("question serializes"),
    )
}

fn status_of(error: &ClientError) -> (u16, String) {
    match error {
        ClientError::Status { status, body } => (*status, body.clone()),
        other => panic!("expected ClientError::Status, got {other:?}"),
    }
}

// -- happy path --

/// The end-to-end path a production caller takes: a `200` is read off the socket
/// and deserialized into a real [`SystemOneResponse`].
#[tokio::test]
async fn a_successful_response_is_parsed_into_a_system_one_response() {
    let server = MockBackend::start(&[(200, system_one_body())]).await;

    let response = client_for(&server.url)
        .evaluate(request())
        .await
        .expect("a 200 must parse");

    assert_eq!(response.model, "jev-1.13.0");
    assert_eq!(
        response.usage.map(|usage| usage.input_tokens),
        Some(120),
        "the optional usage block must survive the round trip"
    );

    let answer = take_answer(&response, "severity").expect("severity must be answered");
    let Answer::Score(score) = answer else {
        panic!("expected a score answer, got {answer:?}");
    };
    assert!((score.score - 1.43).abs() < 1e-9);
    assert!((score.confidence - 0.35).abs() < 1e-9);
    assert_eq!(score.probabilities["2"], 0.43);
}

/// The request must match the documented wire shape, not merely succeed: a
/// silently malformed request would come back `200` with the wrong answer.
#[tokio::test]
async fn the_request_matches_the_documented_api_shape() {
    let server = MockBackend::start(&[(200, system_one_body())]).await;
    client_for(&server.url)
        .evaluate(request())
        .await
        .expect("a 200 must parse");

    let sent = server.only_request();
    assert!(
        sent.request_line.starts_with("POST / HTTP/1.1"),
        "System One is a POST; got {:?}",
        sent.request_line
    );
    assert_eq!(
        sent.header("authorization"),
        Some("Bearer test-key"),
        "the API key must travel as bearer auth"
    );
    assert_eq!(sent.header("content-type"), Some("application/json"));

    let body = sent.json();
    assert_eq!(
        body["state"],
        json!("The export button crashes the settings page.")
    );
    assert_eq!(body["model"], json!("jev-latest"));
    assert_eq!(body["questions"]["severity"]["type"], json!("score"));
    assert_eq!(
        body["questions"]["severity"]["instructions"],
        json!("How severe?")
    );
    assert_eq!(
        body["questions"]["severity"]["criteria"],
        json!(["low", "high"])
    );
}

// -- status errors through the real producing path --

/// `ClientError::Status` has to be produced by the adapter, not hand-built by a
/// test. A `401` also must not be retried: the key is wrong, and resending the
/// identical request only delays the error.
#[tokio::test]
async fn an_unauthorized_response_becomes_a_status_error_without_a_retry() {
    let server = MockBackend::start(&[(401, r#"{"error":"invalid api key"}"#.to_owned())]).await;

    let error = client_for(&server.url)
        .evaluate(request())
        .await
        .expect_err("a 401 is a failure");

    let (status, body) = status_of(&error);
    assert_eq!(status, 401);
    assert!(
        body.contains("invalid api key"),
        "the backend's own words must survive into the error: {body}"
    );
    assert!(
        !error.is_transient(),
        "a 401 is a client error and must not be retried"
    );
    assert_eq!(server.requests().len(), 1, "a bad key must not be retried");
}

/// The heart of finding #6: a `500` is a *server* error, so it is retried, and
/// the retry must resend the identical request rather than a degraded one.
#[tokio::test]
async fn a_server_error_is_retried_and_the_next_attempt_can_succeed() {
    let server = MockBackend::start(&[
        (500, r#"{"error":"upstream unavailable"}"#.to_owned()),
        (200, system_one_body()),
    ])
    .await;

    let response = client_for(&server.url)
        .evaluate(request())
        .await
        .expect("a transient 500 must be retried into a success");

    assert_eq!(response.model, "jev-1.13.0");
    let requests = server.requests();
    assert_eq!(
        requests.len(),
        2,
        "the 500 must actually be retried, not surfaced as a hard failure"
    );
    assert_eq!(
        requests[0].body, requests[1].body,
        "a retry must resend the same request"
    );
}

/// `429` is a `4xx` that clears, so it retries — the documented rate limit.
#[tokio::test]
async fn a_rate_limit_is_retried() {
    let server = MockBackend::start(&[
        (429, r#"{"error":"slow down"}"#.to_owned()),
        (200, system_one_body()),
    ])
    .await;

    client_for(&server.url)
        .evaluate(request())
        .await
        .expect("a 429 must be retried into a success");

    assert_eq!(server.requests().len(), 2, "a 429 must be retried");
}

/// Retry is bounded. When the backend never recovers the caller gets the real
/// status, having spent exactly the attempt budget and no more.
#[tokio::test]
async fn a_server_error_that_never_clears_surfaces_as_a_status_error() {
    let server = MockBackend::start(&[(503, r#"{"error":"still down"}"#.to_owned())]).await;

    let error = client_for(&server.url)
        .evaluate(request())
        .await
        .expect_err("a permanent 503 must eventually surface");

    let (status, _) = status_of(&error);
    assert_eq!(status, 503);
    assert_eq!(
        server.requests().len(),
        3,
        "the 3-attempt budget must be respected exactly"
    );
}

/// A long error body must be capped. A backend that returns a stack trace
/// cannot be allowed to push an unbounded string into every log line that
/// mentions the failure.
#[tokio::test]
async fn a_long_error_body_is_truncated() {
    // Three-byte characters, so a 512-byte cap lands mid-character and the
    // truncation has to walk back to a char boundary rather than panic.
    let body = "€".repeat(2_000);
    let server = MockBackend::start(&[(401, body.clone())]).await;

    let error = client_for(&server.url)
        .evaluate(request())
        .await
        .expect_err("a 401 is a failure");

    let (_, reported) = status_of(&error);
    assert!(
        reported.len() <= 512,
        "an error body must be capped at 512 bytes, got {}",
        reported.len()
    );
    assert!(
        reported.len() < body.len(),
        "a long body must actually be shortened"
    );
    assert!(
        body.starts_with(reported.as_str()),
        "truncation must keep the leading prefix verbatim"
    );
}

// -- the other two error variants, through the real path --

/// A `200` carrying a body that is not the documented shape is a decode failure.
/// Handing back an empty result instead would route on nothing.
#[tokio::test]
async fn a_two_hundred_with_an_undocumented_body_is_a_decode_error() {
    let server = MockBackend::start(&[(200, r#"{"unexpected":true}"#.to_owned())]).await;

    let error = client_for(&server.url)
        .evaluate(request())
        .await
        .expect_err("an unparseable 200 must fail");

    match error {
        ClientError::Decode(message) => {
            assert!(
                !message.is_empty(),
                "the parse error must say what went wrong"
            );
        }
        other => panic!("expected ClientError::Decode, got {other:?}"),
    }
    assert_eq!(
        server.requests().len(),
        1,
        "the same bytes will decode the same way, so a decode failure must not retry"
    );
}

/// A refused connection is the most transient failure there is.
#[tokio::test]
async fn an_unreachable_backend_is_a_transport_error() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback must be bindable");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    drop(listener);

    let error = HttpJudgmentClient::new(&url, "test-key")
        .expect("a loopback URL is valid")
        .with_retry(RetryPolicy {
            attempts: 2,
            base_delay: Duration::from_millis(1),
        })
        .evaluate(request())
        .await
        .expect_err("a closed port cannot answer");

    assert!(
        error.is_transient(),
        "a refused connection must be classified transient"
    );
    match error {
        ClientError::Transport(_) => {}
        other => panic!("expected ClientError::Transport, got {other:?}"),
    }
}

/// The response-size ceiling bounds a hostile or malformed reply, and it must
/// report the ceiling rather than a vague decode failure.
#[tokio::test]
async fn a_response_over_the_ceiling_is_rejected() {
    let server = MockBackend::start(&[(200, system_one_body())]).await;

    let error = client_for(&server.url)
        .with_max_response_bytes(32)
        .evaluate(request())
        .await
        .expect_err("a body over the ceiling must be refused");

    let message = error.to_string();
    assert!(
        message.contains("ceiling"),
        "the error must name the ceiling, got: {message}"
    );
    match error {
        ClientError::Transport(_) => {}
        other => panic!("expected the ceiling to report as Transport, got {other:?}"),
    }
}

// -- construction (#19) --

/// The endpoint is a `&str`, so a downstream caller never has to name — or take
/// a dependency on — `reqwest` just to construct this client. The path is
/// carried through to the request line.
#[tokio::test]
async fn the_endpoint_is_taken_as_a_string_and_may_carry_a_path() {
    let server = MockBackend::start(&[(200, system_one_body())]).await;
    let endpoint = format!("{}/v1/systemone", server.url);

    HttpJudgmentClient::new(&endpoint, "test-key")
        .expect("a &str endpoint must be accepted")
        .evaluate(request())
        .await
        .expect("a 200 must parse");

    assert!(
        server
            .only_request()
            .request_line
            .starts_with("POST /v1/systemone "),
        "the endpoint path must be preserved"
    );
}

/// A malformed endpoint fails at construction, before any socket is opened.
#[tokio::test]
async fn a_malformed_endpoint_is_rejected() {
    let error = HttpJudgmentClient::new("not a url", "test-key")
        .expect_err("an unparseable endpoint must be rejected");
    assert!(
        error.to_string().contains("endpoint"),
        "the error must name the endpoint, got: {error}"
    );
}

/// An empty or whitespace-only key is rejected up front. A blank bearer token
/// would otherwise be sent and come back as an opaque `401` from the backend.
#[tokio::test]
async fn an_empty_or_blank_api_key_is_rejected_before_any_request() {
    for key in ["", "   ", "\t\n"] {
        let error = HttpJudgmentClient::new("http://127.0.0.1:1/v1/systemone", key)
            .expect_err("a blank key must be rejected");
        assert!(
            error.to_string().contains("API key is empty"),
            "key {key:?} must be rejected as empty, got: {error}"
        );
    }
}
