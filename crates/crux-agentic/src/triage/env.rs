//! Handlers for environment probe parsing, severity classification, remediation,
//! failure correlation, and hook overhead measurement.

use crux_script::{HandlerMetadata, HandlerOutput, HandlerRegistry, RiskLevel};
use serde_json::{Value, json};

use crate::handlers;

// Confidence scores for secret-chain classification
const CONFIDENCE_BROKEN_SECRETS: f32 = 0.1;
const CONFIDENCE_DIRENV_UNLOADED: f32 = 0.3;
const CONFIDENCE_KEY_MISSING: f32 = 0.6;
const CONFIDENCE_HEALTHY_SECRETS: f32 = 0.95;

// Hook overhead latency ceiling (ms) for confidence degradation
const MAX_HOOK_OVERHEAD_MS: f64 = 5000.0;

pub(super) fn register(registry: &mut HandlerRegistry) {
    register_parse_env_probe(registry);
    register_classify_severity(registry);
    register_suggest_remediation(registry);
    register_correlate_failures(registry);
    register_measure_overhead(registry);
}

fn register_parse_env_probe(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_PARSE_ENV_PROBE)
            .describe("Parse 1Password, direnv, and dotenvx probe outputs into a findings list.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler_value(handlers::TRIAGE_PARSE_ENV_PROBE, |input: Value| async move {
        let mut findings = Vec::new();

        let op_output = input
            .pointer("/check_op_auth/output")
            .or_else(|| input.pointer("/probe_chain/check_op_auth/output"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if op_output.is_empty() || op_output.contains("error") || op_output.contains("FAILED") {
            findings.push(json!({"component": "1password", "status": "broken",
                "detail": "op account list returned no accounts or an error"}));
        } else {
            findings.push(json!({"component": "1password", "status": "ok"}));
        }

        let direnv_output = input
            .pointer("/check_direnv/output")
            .or_else(|| input.pointer("/probe_chain/check_direnv/output"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if direnv_output.contains("not loaded") || direnv_output.is_empty() {
            findings.push(json!({"component": "direnv", "status": "unloaded",
                "detail": "direnv is not active in this shell"}));
        } else {
            findings.push(json!({"component": "direnv", "status": "ok"}));
        }

        let dotenvx_output = input
            .pointer("/check_dotenvx/output")
            .or_else(|| input.pointer("/probe_chain/check_dotenvx/output"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let key_present = dotenvx_output.contains("DOTENV_PRIVATE_KEY=set");
        findings.push(json!({
            "component": "dotenvx",
            "status": if key_present { "ok" } else { "missing_key" },
            "detail": if key_present { "private key present" } else { "DOTENV_PRIVATE_KEY not set" },
        }));

        Ok(json!({"findings": findings}))
    });
}

fn register_classify_severity(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_CLASSIFY_SEVERITY)
            .describe("Classify secret-chain health and emit a confidence score.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(handlers::TRIAGE_CLASSIFY_SEVERITY, |input: Value| async move {
        let findings = input
            .get("findings")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();

        // Order: 1password > direnv > dotenvx key
        let broken = findings
            .iter()
            .filter(|f| f.get("status").and_then(|s| s.as_str()) == Some("broken"))
            .count();
        let unloaded = findings
            .iter()
            .filter(|f| f.get("status").and_then(|s| s.as_str()) == Some("unloaded"))
            .count();
        let missing = findings
            .iter()
            .filter(|f| f.get("status").and_then(|s| s.as_str()) == Some("missing_key"))
            .count();

        let confidence: f32 = match (broken, unloaded, missing) {
            (b, _, _) if b > 0 => CONFIDENCE_BROKEN_SECRETS,
            (_, u, _) if u > 0 => CONFIDENCE_DIRENV_UNLOADED,
            (_, _, m) if m > 0 => CONFIDENCE_KEY_MISSING,
            _ => CONFIDENCE_HEALTHY_SECRETS,
        };

        Ok(HandlerOutput::with_confidence(
            json!({"findings": findings, "broken": broken, "unloaded": unloaded, "missing": missing}),
            confidence,
        ))
    });
}

fn register_suggest_remediation(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_SUGGEST_REMEDIATION)
            .describe("Suggest fix commands for broken 1Password, direnv, or dotenvx components.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let mut fixes = Vec::new();

            if input.get("broken").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                fixes.push(json!({
                    "component": "1password",
                    "fix": "Open 1Password and unlock it, then retry: op account list"
                }));
            }
            if input.get("unloaded").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                fixes.push(json!({
                    "component": "direnv",
                    "fix": "cd $HOME/dev && direnv allow"
                }));
            }
            if input.get("missing").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                fixes.push(json!({
                    "component": "dotenvx",
                    "fix": "export DOTENV_PRIVATE_KEY=$(op read 'op://Personal/nihl7o2bojy53zy4aqtr7txyqi/password')"
                }));
            }
            if fixes.is_empty() {
                fixes.push(json!({"component": "all", "fix": "Chain is healthy — no action needed"}));
            }

            Ok(json!({"fixes": fixes}))
        },
    );
}

fn register_correlate_failures(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_CORRELATE_FAILURES)
            .describe("Correlate hook names against recent failure text to identify culprits.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let hooks = input
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let failure_text = input
                .pointer("/recent_failures/output")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let correlated: Vec<Value> = hooks
                .iter()
                .filter_map(|h| {
                    let name = h.as_str().unwrap_or("");
                    if failure_text.contains(name) {
                        Some(json!({"hook": name, "has_failures": true}))
                    } else {
                        None
                    }
                })
                .collect();

            Ok(json!({"correlated_failures": correlated, "total_hooks": hooks.len()}))
        },
    );
}

fn register_measure_overhead(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_MEASURE_OVERHEAD)
            .describe(
                "Compute p50/p95 latency from hook duration samples and emit a confidence score.",
            )
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(
        handlers::TRIAGE_MEASURE_OVERHEAD,
        |input: Value| async move {
            let items = input
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let durations: Vec<f64> = items
                .iter()
                .filter_map(|item| {
                    item.get("duration_ms")
                        .or_else(|| item.get("elapsed_ms"))
                        .and_then(|v| v.as_f64())
                })
                .collect();

            let (p50, p95) = if durations.is_empty() {
                (0.0, 0.0)
            } else {
                let mut sorted = durations.clone();
                sorted.sort_by(|a, b| a.total_cmp(b));
                let p50 = sorted[sorted.len() / 2];
                let p95 = sorted[(sorted.len() * 95) / 100];
                (p50, p95)
            };

            // confidence: 1.0 if p95 < 500ms, degrades linearly to 0.0 at MAX_HOOK_OVERHEAD_MS
            let confidence = (1.0 - (p95 / MAX_HOOK_OVERHEAD_MS).min(1.0)) as f32;

            Ok(HandlerOutput::with_confidence(
                json!({"p50_ms": p50, "p95_ms": p95, "sample_count": durations.len()}),
                confidence,
            ))
        },
    );
}
