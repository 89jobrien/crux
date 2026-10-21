//! Minimal test plugin: declares one handler "echo::reflect" that
//! returns its input unchanged.

use std::io::{self, BufRead, Write};

fn main() {
    let handler_name = std::env::var("ECHO_HANDLER").unwrap_or_else(|_| "echo::reflect".to_owned());
    let delay = std::env::var("ECHO_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(std::time::Duration::from_millis);
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: serde_json::Value = serde_json::from_str(&line).expect("invalid JSON");

        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

        let resp = match method {
            "Declare" => {
                // `ECHO_OUTPUT_SCHEMA` carries a raw JSON `ValueSchema`. Absent,
                // the declaration omits the field so the host types the handler
                // as `Dynamic` -- the shape of a plugin predating the field.
                let output_schema = std::env::var("ECHO_OUTPUT_SCHEMA").ok();
                let mut decl = serde_json::json!({
                    "name": handler_name,
                    "description": "Returns input unchanged"
                });
                if let Some(raw) = output_schema {
                    let schema: serde_json::Value =
                        serde_json::from_str(&raw).expect("ECHO_OUTPUT_SCHEMA must be JSON");
                    decl["output_schema"] = schema;
                }
                serde_json::json!({
                    "status": "Declare",
                    "data": { "handlers": [decl] }
                })
            }
            "Invoke" => {
                if std::env::var("ECHO_CRASH_ON_INVOKE").is_ok() {
                    // Simulate a process crash mid-call: exit without a
                    // response so the host observes an I/O failure.
                    std::process::exit(1);
                }
                if let Ok(marker) = std::env::var("ECHO_CRASH_ONCE_FILE") {
                    // Crash exactly once across restarts of the same
                    // marker file: the first process to see it missing
                    // creates it and exits; any later process (e.g. one
                    // spawned by the host's automatic restart) finds the
                    // marker and behaves normally.
                    if !std::path::Path::new(&marker).exists() {
                        let _ = std::fs::write(&marker, b"crashed");
                        std::process::exit(1);
                    }
                }
                if let Some(delay) = delay {
                    std::thread::sleep(delay);
                }
                let input = req
                    .get("params")
                    .and_then(|p| p.get("input"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "status": "InvokeOk",
                    "data": { "output": input }
                })
            }
            "Shutdown" => {
                let resp = serde_json::json!({ "status": "ShutdownAck" });
                serde_json::to_writer(&mut out, &resp).ok();
                writeln!(out).ok();
                out.flush().ok();
                break;
            }
            _ => serde_json::json!({
                "status": "InvokeErr",
                "data": { "error": format!("unknown method: {method}") }
            }),
        };

        serde_json::to_writer(&mut out, &resp).unwrap();
        writeln!(out).unwrap();
        out.flush().unwrap();
    }
}
