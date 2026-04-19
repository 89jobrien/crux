//! Minimal test plugin: declares one handler "echo::reflect" that
//! returns its input unchanged.

use std::io::{self, BufRead, Write};

fn main() {
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

        let method = req
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let resp = match method {
            "Declare" => serde_json::json!({
                "status": "Declare",
                "data": {
                    "handlers": [
                        {
                            "name": "echo::reflect",
                            "description": "Returns input unchanged"
                        }
                    ]
                }
            }),
            "Invoke" => {
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
