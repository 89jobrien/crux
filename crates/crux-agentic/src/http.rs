//! Policy-aware typed HTTP handler.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

use crux_runtime::prelude::CruxErr;
use crux_script::{
    ArgSchema, ArgType, Capability, HandlerMetadata, HandlerRegistry, RiskLevel, SideEffect,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub const HTTP_REQUEST: &str = "http::request";
const DEFAULT_RESPONSE_LIMIT: usize = 1024 * 1024;
const MAX_RESPONSE_LIMIT: usize = 16 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Deserialize)]
struct HttpArgs {
    method: String,
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<Value>,
    #[serde(default = "default_response_limit")]
    max_response_bytes: usize,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    network_policy: NetworkPolicy,
}

#[derive(Debug, Deserialize)]
struct NetworkPolicy {
    allowed_hosts: Vec<String>,
    #[serde(default)]
    allow_http: bool,
    #[serde(default)]
    allow_private_networks: bool,
}

const fn default_response_limit() -> usize {
    DEFAULT_RESPONSE_LIMIT
}

const fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(HTTP_REQUEST)
            .describe("Send a bounded HTTP request subject to an explicit network policy.")
            .args(
                ArgSchema::new()
                    .required("method", ArgType::String)
                    .required("url", ArgType::String)
                    .required("network_policy", ArgType::Object)
                    .optional("headers", ArgType::Object)
                    .optional("body", ArgType::Any)
                    .optional("max_response_bytes", ArgType::Integer)
                    .optional("timeout_ms", ArgType::Integer),
            )
            .risk(RiskLevel::Medium)
            .side_effects(vec![SideEffect::Network])
            .capabilities(vec![Capability::Network])
            .deterministic(false),
        |input: Value| async move { execute(input).await },
    );
}

async fn execute(input: Value) -> Result<Value, CruxErr> {
    let args = input
        .get("args")
        .cloned()
        .ok_or_else(|| failure("missing args object"))?;
    let args: HttpArgs =
        serde_json::from_value(args).map_err(|error| failure(format!("invalid args: {error}")))?;
    validate_limits(&args)?;

    let url =
        reqwest::Url::parse(&args.url).map_err(|error| failure(format!("invalid URL: {error}")))?;
    args.network_policy.validate(&url)?;
    let method = reqwest::Method::from_bytes(args.method.as_bytes())
        .map_err(|error| failure(format!("invalid HTTP method: {error}")))?;
    let timeout = Duration::from_millis(args.timeout_ms);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|error| failure(format!("failed to build HTTP client: {error}")))?;
    let mut request = client.request(method, url).timeout(timeout);
    for (name, value) in args.headers {
        request = request.header(&name, &value);
    }
    if let Some(body) = args.body {
        request = match body {
            Value::String(text) => request.body(text),
            value => request.json(&value),
        };
    }

    let mut response = request
        .send()
        .await
        .map_err(|error| failure(format!("request failed: {error}")))?;
    if response
        .content_length()
        .is_some_and(|length| length > args.max_response_bytes as u64)
    {
        return Err(failure(format!(
            "response limit of {} bytes exceeded",
            args.max_response_bytes
        )));
    }

    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                Value::String(value.to_str().unwrap_or_default().to_owned()),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| failure(format!("failed to read response: {error}")))?
    {
        if body.len().saturating_add(chunk.len()) > args.max_response_bytes {
            return Err(failure(format!(
                "response limit of {} bytes exceeded",
                args.max_response_bytes
            )));
        }
        body.extend_from_slice(&chunk);
    }

    Ok(json!({
        "status": status,
        "headers": headers,
        "body": String::from_utf8_lossy(&body),
    }))
}

fn validate_limits(args: &HttpArgs) -> Result<(), CruxErr> {
    if args.max_response_bytes == 0 || args.max_response_bytes > MAX_RESPONSE_LIMIT {
        return Err(failure(format!(
            "max_response_bytes must be between 1 and {MAX_RESPONSE_LIMIT}"
        )));
    }
    if args.timeout_ms == 0 || args.timeout_ms > MAX_TIMEOUT_MS {
        return Err(failure(format!(
            "timeout_ms must be between 1 and {MAX_TIMEOUT_MS}"
        )));
    }
    Ok(())
}

impl NetworkPolicy {
    fn validate(&self, url: &reqwest::Url) -> Result<(), CruxErr> {
        let scheme = url.scheme();
        if scheme != "https" && !(scheme == "http" && self.allow_http) {
            return Err(failure(format!("URL scheme '{scheme}' is not allowed")));
        }
        let host = url
            .host_str()
            .ok_or_else(|| failure("URL must include a host"))?;
        if !self.allowed_hosts.iter().any(|allowed| {
            host.eq_ignore_ascii_case(allowed)
                || allowed
                    .strip_prefix("*.")
                    .is_some_and(|suffix| host.ends_with(&format!(".{suffix}")))
        }) {
            return Err(failure(format!("host '{host}' is not allowed")));
        }
        if !self.allow_private_networks && host.parse::<IpAddr>().is_ok_and(is_private_address) {
            return Err(failure(format!(
                "private network host '{host}' is not allowed"
            )));
        }
        Ok(())
    }
}

fn is_private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
        }
        IpAddr::V6(address) => {
            address.is_loopback() || address.is_unspecified() || address.is_unique_local()
        }
    }
}

fn failure(message: impl Into<String>) -> CruxErr {
    CruxErr::step_failed(HTTP_REQUEST, message)
}
