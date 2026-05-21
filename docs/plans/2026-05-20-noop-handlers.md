# Plan: Fill In Noop Pipeline Placeholders

## Goal

Replace all 31 `ctrl::noop` placeholders across 5 example pipelines with real
handler implementations in `crux-agentic`, organized into 4 new modules.

## Architecture

- **Crate**: `crux-agentic` (all new code lives here)
- **New modules**: `analysis.rs`, `ci.rs`, `review.rs`, `triage.rs`
- **No new dependencies**: all handlers use `serde_json`, `tokio::process::Command`,
  and existing `crux_runtime::prelude::CruxErr` / `crux_script` types
- **Data flow**: pipeline YAML `args` -> handler `input: Value` -> pure JSON transform
  or `shell::capture`-style subprocess -> `Result<Value, CruxErr>`
- **Confidence**: handlers that feed `route_on_confidence` or `speculate: pick_best`
  use `registry.handler()` + `HandlerOutput::with_confidence()`; all others use
  `registry.handler_value()`

## Files to Create

| File                                       | Purpose                     |
| ------------------------------------------ | --------------------------- |
| `crates/crux-agentic/src/analysis.rs`      | Trace analysis handlers (9) |
| `crates/crux-agentic/src/ci.rs`            | CI log parsing handlers (8) |
| `crates/crux-agentic/src/review.rs`        | PR review handlers (5)      |
| `crates/crux-agentic/src/triage.rs`        | Doob triage handlers (4)    |
| `crates/crux-agentic/tests/analysis.rs`    | Tests for analysis handlers |
| `crates/crux-agentic/tests/ci_handlers.rs` | Tests for CI handlers       |
| `crates/crux-agentic/tests/review.rs`      | Tests for review handlers   |
| `crates/crux-agentic/tests/triage.rs`      | Tests for triage handlers   |

## Files to Modify

| File                                        | Change                                 |
| ------------------------------------------- | -------------------------------------- |
| `crates/crux-agentic/src/lib.rs`            | Add `pub mod` + `register()` calls     |
| `crates/crux-agentic/src/handlers.rs`       | Add 26 handler name constants          |
| `crates/crux-agentic/tests/register_all.rs` | Add new handlers to expected list      |
| `examples/joe/agent_meta_eval.crux`         | Replace 9 noops                        |
| `examples/joe/ci_triage.yaml`               | Replace 8 noops                        |
| `examples/joe/crate_refactor.yaml`          | Replace 6 noops                        |
| `examples/joe/pr_review.crux`               | Replace 5 noops                        |
| `examples/joe/doob_triage.yaml`             | Replace 4 noops                        |
| `docs/crux-capabilities.md`                 | Add new handlers, remove resolved gaps |

## Handler Inventory

### analysis.rs (9 handlers)

All operate on a JSON array of Step objects (trace input).

| Handler                        | Registration    | Input                                             | Output                                                                          |
| ------------------------------ | --------------- | ------------------------------------------------- | ------------------------------------------------------------------------------- |
| `analysis::latency_profile`    | `handler_value` | `[Step]` array with `started_at`/`completed_at`   | `{ slow_steps: [{name, duration_ms, ratio_to_median}], median_ms }`             |
| `analysis::token_spend`        | `handler_value` | `[Step]` array with `output.metadata.tokens`      | `{ by_step: [{name, tokens}], total, top3: [name] }`                            |
| `analysis::failure_clusters`   | `handler_value` | `[Step]` array                                    | `{ clusters: [{kind, count, step_names: []}] }`                                 |
| `analysis::replay_cache_hits`  | `handler_value` | `[Step]` array with `cache_hit` field             | `{ by_step: [{name, hits, misses, ratio}] }`                                    |
| `analysis::tighten_budget`     | `handler`       | `{ token_spend, budget }` merged from prior steps | `HandlerOutput::with_confidence(patch, score)` where score = spend/budget ratio |
| `analysis::compress_stages`    | `handler_value` | `{ latency_profile, token_spend }`                | `{ suggestions: [{stage, reason, action}] }`                                    |
| `analysis::tune_retry`         | `handler_value` | `{ failure_clusters }`                            | `{ suggestions: [{step_name, retry_count, backoff_ms}] }`                       |
| `analysis::patch_schema_check` | `handler_value` | `{ patch }` (YAML string)                         | `{ valid: bool, errors: [] }` via `crux check` subprocess                       |
| `analysis::replay_dry_run`     | `handler_value` | `{ patch, trace_path }`                           | `{ ok: bool, mismatches: [] }` via `crux replay` subprocess                     |

### ci.rs (8 handlers)

All parse CI log text (string input from `shell::capture` output).

| Handler                 | Input                              | Output                                                     |
| ----------------------- | ---------------------------------- | ---------------------------------------------------------- |
| `ci::compile_errors`    | CI log text                        | `{ errors: [{code, message, file, line}] }`                |
| `ci::clippy_violations` | CI log text                        | `{ violations: [{lint, message, file, line}] }`            |
| `ci::nextest_failures`  | CI log text                        | `{ failures: [{test_name, message, file}] }`               |
| `ci::deny_violations`   | CI log text                        | `{ violations: [{kind, crate_name, message}] }`            |
| `ci::deduplicate_spans` | `{ errors, violations, failures }` | Same shape, deduplicated by file+line                      |
| `ci::classify_severity` | Deduplicated findings              | `{ ranked: [{..., severity}] }` (compile/deny/test/clippy) |
| `ci::attach_owners`     | Ranked findings                    | `{ ranked: [{..., crate_name}] }` via `cargo metadata`     |
| `ci::score_fixability`  | Ranked+owners                      | `HandlerOutput::with_confidence(findings, score)`          |

### review.rs (5 handlers)

| Handler                       | Registration    | Input                        | Output                                                          |
| ----------------------------- | --------------- | ---------------------------- | --------------------------------------------------------------- |
| `review::arch_boundary_check` | `handler_value` | `{ files: [path] }`          | `{ violations: [{file, imports, violation}] }` via `rg`         |
| `review::normalize_findings`  | `handler_value` | `{ clippy, arch, coverage }` | `{ findings: [{source, file, line, message, severity}] }`       |
| `review::apply_severity`      | `handler_value` | `{ findings }`               | `{ findings: [{..., tier}] }` (blocking/suggestion/observation) |
| `review::compute_score`       | `handler`       | `{ findings }`               | `HandlerOutput::with_confidence(summary, score)`                |
| `review::approve`             | `handler_value` | Input passthrough            | Runs `gh pr review --approve` via subprocess                    |

### triage.rs (4 handlers)

| Handler                      | Input                    | Output                                                                  |
| ---------------------------- | ------------------------ | ----------------------------------------------------------------------- |
| `triage::parse_repo_tags`    | JSON array of doob todos | `[{..., repo: "extracted"}]`                                            |
| `triage::score_urgency`      | Tagged todos             | `[{..., urgency_score: f64}]` sorted desc                               |
| `triage::deduplicate_intent` | Scored todos             | `{ groups: [{canonical, duplicates: [id]}] }` — edit-distance heuristic |
| `triage::group_by_repo`      | Deduplicated todos       | `{ repos: { "repo_name": [todo] } }`                                    |

### crate_refactor.yaml — mapped to existing + new handlers

| Noop                  | Replacement                                                                 |
| --------------------- | --------------------------------------------------------------------------- |
| `dep_graph_analysis`  | `review::arch_boundary_check` (reuse — operates on `cargo metadata` output) |
| `arch_boundary_check` | `review::arch_boundary_check` (same handler)                                |
| `extract_trait_port`  | `llm::invoke` with args for trait extraction prompt                         |
| `split_crate`         | `llm::invoke` with args for scaffold generation prompt                      |
| `add_adapter_layer`   | `llm::invoke` with args for adapter scaffold prompt                         |
| `generate_patch`      | `llm::invoke` with args for diff generation prompt                          |

These 6 don't need new Rust handlers — they reuse `review::arch_boundary_check`
(2 slots) and `llm::invoke` (4 slots) with pipeline-level `args`.

## Tasks

### Task 1: Create `analysis.rs` module

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/analysis.rs`
**Run**: `cargo nextest run -p crux-agentic -- analysis`

1. Write failing test in `crates/crux-agentic/tests/analysis.rs`:

   ```rust
   use crux_script::HandlerRegistry;
   use serde_json::json;

   fn registry() -> HandlerRegistry {
       let mut r = HandlerRegistry::new();
       crux_agentic::analysis::register(&mut r);
       r
   }

   #[tokio::test]
   async fn latency_profile_flags_slow_steps() {
       let reg = registry();
       let h = reg.get_handler("analysis::latency_profile").unwrap();
       let input = json!({
           "steps": [
               {"name": "a", "started_at": "2026-01-01T00:00:00Z",
                "completed_at": "2026-01-01T00:00:01Z"},
               {"name": "b", "started_at": "2026-01-01T00:00:00Z",
                "completed_at": "2026-01-01T00:00:10Z"},
               {"name": "c", "started_at": "2026-01-01T00:00:00Z",
                "completed_at": "2026-01-01T00:00:01Z"},
           ]
       });
       let out = h(input).await.unwrap();
       let slow = out.value["slow_steps"].as_array().unwrap();
       assert_eq!(slow.len(), 1);
       assert_eq!(slow[0]["name"], "b");
   }

   #[tokio::test]
   async fn token_spend_top3() {
       let reg = registry();
       let h = reg.get_handler("analysis::token_spend").unwrap();
       let input = json!({
           "steps": [
               {"name": "a", "output": {"metadata": {"tokens": 100}}},
               {"name": "b", "output": {"metadata": {"tokens": 500}}},
               {"name": "c", "output": {"metadata": {"tokens": 200}}},
               {"name": "d", "output": {"metadata": {"tokens": 50}}},
           ]
       });
       let out = h(input).await.unwrap();
       assert_eq!(out.value["total"], 850);
       let top3 = out.value["top3"].as_array().unwrap();
       assert_eq!(top3[0], "b");
   }

   #[tokio::test]
   async fn failure_clusters_groups_by_kind() {
       let reg = registry();
       let h = reg.get_handler("analysis::failure_clusters").unwrap();
       let input = json!({
           "steps": [
               {"name": "x", "status": "failed",
                "error": {"kind": "StepFailed"}},
               {"name": "y", "status": "failed",
                "error": {"kind": "StepFailed"}},
               {"name": "z", "status": "failed",
                "error": {"kind": "Timeout"}},
           ]
       });
       let out = h(input).await.unwrap();
       let clusters = out.value["clusters"].as_array().unwrap();
       assert_eq!(clusters.len(), 2);
   }

   #[tokio::test]
   async fn replay_cache_hit_ratio() {
       let reg = registry();
       let h = reg.get_handler("analysis::replay_cache_hits").unwrap();
       let input = json!({
           "steps": [
               {"name": "a", "cache_hit": true},
               {"name": "a", "cache_hit": false},
               {"name": "b", "cache_hit": true},
               {"name": "b", "cache_hit": true},
           ]
       });
       let out = h(input).await.unwrap();
       let by_step = out.value["by_step"].as_array().unwrap();
       let a = by_step.iter().find(|s| s["name"] == "a").unwrap();
       assert_eq!(a["hits"], 1);
       assert_eq!(a["misses"], 1);
   }

   #[tokio::test]
   async fn tighten_budget_emits_confidence() {
       let reg = registry();
       let h = reg.get_handler("analysis::tighten_budget").unwrap();
       let input = json!({
           "args": {},
           "token_spend": {"total": 900},
           "budget": {"tokens": 1000}
       });
       let out = h(input).await.unwrap();
       assert!(out.confidence.is_some());
       // 900/1000 = 0.9 > 0.8 threshold, should emit a suggestion
       assert!(out.value.get("suggestion").is_some());
   }

   #[tokio::test]
   async fn tighten_budget_skips_when_under_threshold() {
       let reg = registry();
       let h = reg.get_handler("analysis::tighten_budget").unwrap();
       let input = json!({
           "args": {},
           "token_spend": {"total": 500},
           "budget": {"tokens": 1000}
       });
       let out = h(input).await.unwrap();
       assert!(out.value.get("suggestion").is_none());
   }

   #[tokio::test]
   async fn tune_retry_suggests_backoff() {
       let reg = registry();
       let h = reg.get_handler("analysis::tune_retry").unwrap();
       let input = json!({
           "failure_clusters": {
               "clusters": [
                   {"kind": "StepFailed", "count": 3,
                    "step_names": ["flaky_step"]},
                   {"kind": "Timeout", "count": 1,
                    "step_names": ["ok_step"]},
               ]
           }
       });
       let out = h(input).await.unwrap();
       let suggestions = out.value["suggestions"].as_array().unwrap();
       assert_eq!(suggestions.len(), 1);
       assert_eq!(suggestions[0]["step_name"], "flaky_step");
   }

   #[tokio::test]
   async fn compress_stages_flags_heavy_stages() {
       let reg = registry();
       let h = reg.get_handler("analysis::compress_stages").unwrap();
       let input = json!({
           "token_spend": {
               "by_step": [
                   {"name": "heavy", "tokens": 500},
                   {"name": "light", "tokens": 100},
               ],
               "total": 600
           }
       });
       let out = h(input).await.unwrap();
       let suggestions = out.value["suggestions"].as_array().unwrap();
       assert_eq!(suggestions.len(), 1);
       assert_eq!(suggestions[0]["stage"], "heavy");
   }
   ```

   Run: `cargo nextest run -p crux-agentic -- analysis`
   Expected: FAIL (module doesn't exist)

2. Implement `crates/crux-agentic/src/analysis.rs`:

   ```rust
   use chrono::{DateTime, Utc};
   use crux_runtime::prelude::CruxErr;
   use crux_script::{HandlerOutput, HandlerRegistry};
   use serde_json::{Map, Value, json};
   use std::collections::HashMap;

   pub fn register(registry: &mut HandlerRegistry) {
       registry.handler_value(
           "analysis::latency_profile",
           |input: Value| async move {
               let steps = input
                   .get("steps")
                   .and_then(|s| s.as_array())
                   .cloned()
                   .unwrap_or_default();

               let mut durations: Vec<(String, f64)> = Vec::new();
               for step in &steps {
                   let name = step
                       .get("name")
                       .and_then(|n| n.as_str())
                       .unwrap_or("unknown");
                   let started = step
                       .get("started_at")
                       .and_then(|s| s.as_str())
                       .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                   let completed = step
                       .get("completed_at")
                       .and_then(|s| s.as_str())
                       .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                   if let (Some(s), Some(c)) = (started, completed) {
                       let ms = (c - s).num_milliseconds() as f64;
                       durations.push((name.to_string(), ms));
                   }
               }

               if durations.is_empty() {
                   return Ok(json!({"slow_steps": [], "median_ms": 0}));
               }

               let mut sorted: Vec<f64> =
                   durations.iter().map(|(_, d)| *d).collect();
               sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
               let median = sorted[sorted.len() / 2];

               let slow: Vec<Value> = durations
                   .iter()
                   .filter(|(_, d)| *d > median * 2.0)
                   .map(|(name, d)| {
                       json!({
                           "name": name,
                           "duration_ms": *d,
                           "ratio_to_median": if median > 0.0 {
                               *d / median
                           } else {
                               0.0
                           }
                       })
                   })
                   .collect();

               Ok(json!({"slow_steps": slow, "median_ms": median}))
           },
       );

       registry.handler_value(
           "analysis::token_spend",
           |input: Value| async move {
               let steps = input
                   .get("steps")
                   .and_then(|s| s.as_array())
                   .cloned()
                   .unwrap_or_default();

               let mut by_step: Vec<(String, u64)> = Vec::new();
               for step in &steps {
                   let name = step
                       .get("name")
                       .and_then(|n| n.as_str())
                       .unwrap_or("unknown")
                       .to_string();
                   let tokens = step
                       .pointer("/output/metadata/tokens")
                       .and_then(|t| t.as_u64())
                       .unwrap_or(0);
                   by_step.push((name, tokens));
               }

               by_step.sort_by(|a, b| b.1.cmp(&a.1));
               let total: u64 = by_step.iter().map(|(_, t)| *t).sum();
               let top3: Vec<&str> = by_step
                   .iter()
                   .take(3)
                   .map(|(n, _)| n.as_str())
                   .collect();
               let items: Vec<Value> = by_step
                   .iter()
                   .map(|(n, t)| json!({"name": n, "tokens": t}))
                   .collect();

               Ok(json!({
                   "by_step": items,
                   "total": total,
                   "top3": top3
               }))
           },
       );

       registry.handler_value(
           "analysis::failure_clusters",
           |input: Value| async move {
               let steps = input
                   .get("steps")
                   .and_then(|s| s.as_array())
                   .cloned()
                   .unwrap_or_default();

               let mut clusters: HashMap<String, Vec<String>> = HashMap::new();
               for step in &steps {
                   let status = step
                       .get("status")
                       .and_then(|s| s.as_str())
                       .unwrap_or("");
                   if status != "failed" {
                       continue;
                   }
                   let kind = step
                       .pointer("/error/kind")
                       .and_then(|k| k.as_str())
                       .unwrap_or("Unknown")
                       .to_string();
                   let name = step
                       .get("name")
                       .and_then(|n| n.as_str())
                       .unwrap_or("unknown")
                       .to_string();
                   clusters.entry(kind).or_default().push(name);
               }

               let items: Vec<Value> = clusters
                   .into_iter()
                   .map(|(kind, names)| {
                       json!({
                           "kind": kind,
                           "count": names.len(),
                           "step_names": names
                       })
                   })
                   .collect();

               Ok(json!({"clusters": items}))
           },
       );

       registry.handler_value(
           "analysis::replay_cache_hits",
           |input: Value| async move {
               let steps = input
                   .get("steps")
                   .and_then(|s| s.as_array())
                   .cloned()
                   .unwrap_or_default();

               let mut stats: HashMap<String, (u64, u64)> = HashMap::new();
               for step in &steps {
                   let name = step
                       .get("name")
                       .and_then(|n| n.as_str())
                       .unwrap_or("unknown")
                       .to_string();
                   let hit = step
                       .get("cache_hit")
                       .and_then(|h| h.as_bool())
                       .unwrap_or(false);
                   let entry = stats.entry(name).or_insert((0, 0));
                   if hit {
                       entry.0 += 1;
                   } else {
                       entry.1 += 1;
                   }
               }

               let items: Vec<Value> = stats
                   .into_iter()
                   .map(|(name, (hits, misses))| {
                       let total = hits + misses;
                       json!({
                           "name": name,
                           "hits": hits,
                           "misses": misses,
                           "ratio": if total > 0 {
                               hits as f64 / total as f64
                           } else {
                               0.0
                           }
                       })
                   })
                   .collect();

               Ok(json!({"by_step": items}))
           },
       );

       registry.handler(
           "analysis::tighten_budget",
           |input: Value| async move {
               let spend = input
                   .pointer("/token_spend/total")
                   .and_then(|t| t.as_f64())
                   .unwrap_or(0.0);
               let budget = input
                   .pointer("/budget/tokens")
                   .and_then(|t| t.as_f64())
                   .unwrap_or(1.0);

               let ratio = if budget > 0.0 {
                   spend / budget
               } else {
                   0.0
               };

               if ratio > 0.8 {
                   let suggested = (spend * 1.1).ceil() as u64;
                   Ok(HandlerOutput::with_confidence(
                       json!({
                           "suggestion": {
                               "current_budget": budget,
                               "actual_spend": spend,
                               "ratio": ratio,
                               "suggested_budget": suggested
                           }
                       }),
                       ratio as f32,
                   ))
               } else {
                   Ok(HandlerOutput::with_confidence(
                       json!({"ratio": ratio}),
                       ratio as f32,
                   ))
               }
           },
       );

       registry.handler_value(
           "analysis::compress_stages",
           |input: Value| async move {
               let by_step = input
                   .pointer("/token_spend/by_step")
                   .and_then(|s| s.as_array())
                   .cloned()
                   .unwrap_or_default();
               let total = input
                   .pointer("/token_spend/total")
                   .and_then(|t| t.as_f64())
                   .unwrap_or(1.0);

               let suggestions: Vec<Value> = by_step
                   .iter()
                   .filter_map(|step| {
                       let tokens = step
                           .get("tokens")
                           .and_then(|t| t.as_f64())
                           .unwrap_or(0.0);
                       let fraction = if total > 0.0 {
                           tokens / total
                       } else {
                           0.0
                       };
                       if fraction > 0.4 {
                           Some(json!({
                               "stage": step.get("name")
                                   .and_then(|n| n.as_str())
                                   .unwrap_or("unknown"),
                               "reason": format!(
                                   "consumes {:.0}% of total tokens",
                                   fraction * 100.0
                               ),
                               "action": "collapse stages or use cheaper model"
                           }))
                       } else {
                           None
                       }
                   })
                   .collect();

               Ok(json!({"suggestions": suggestions}))
           },
       );

       registry.handler_value(
           "analysis::tune_retry",
           |input: Value| async move {
               let clusters = input
                   .pointer("/failure_clusters/clusters")
                   .and_then(|c| c.as_array())
                   .cloned()
                   .unwrap_or_default();

               let suggestions: Vec<Value> = clusters
                   .iter()
                   .filter_map(|cluster| {
                       let count = cluster
                           .get("count")
                           .and_then(|c| c.as_u64())
                           .unwrap_or(0);
                       if count <= 2 {
                           return None;
                       }
                       let names = cluster
                           .get("step_names")
                           .and_then(|n| n.as_array())
                           .cloned()
                           .unwrap_or_default();
                       Some(
                           names
                               .iter()
                               .filter_map(|n| n.as_str())
                               .map(|name| {
                                   json!({
                                       "step_name": name,
                                       "retry_count": 3,
                                       "backoff_ms": 1000
                                   })
                               })
                               .collect::<Vec<Value>>(),
                       )
                   })
                   .flatten()
                   .collect();

               Ok(json!({"suggestions": suggestions}))
           },
       );

       registry.handler_value(
           "analysis::patch_schema_check",
           |input: Value| async move {
               let patch = input
                   .get("patch")
                   .and_then(|p| p.as_str())
                   .unwrap_or("");

               if patch.is_empty() {
                   return Ok(json!({"valid": false,
                       "errors": ["empty patch"]}));
               }

               // Validate YAML syntax as a baseline
               match serde_yaml::from_str::<Value>(patch) {
                   Ok(_) => Ok(json!({"valid": true, "errors": []})),
                   Err(e) => Ok(json!({
                       "valid": false,
                       "errors": [e.to_string()]
                   })),
               }
           },
       );

       registry.handler_value(
           "analysis::replay_dry_run",
           |input: Value| async move {
               let trace_path = input
                   .get("trace_path")
                   .and_then(|p| p.as_str())
                   .unwrap_or("");
               let patch = input
                   .get("patch")
                   .and_then(|p| p.as_str())
                   .unwrap_or("");

               if trace_path.is_empty() || patch.is_empty() {
                   return Ok(json!({
                       "ok": false,
                       "mismatches": ["missing trace_path or patch"]
                   }));
               }

               // Write patch to temp file, run crux replay in lenient mode
               let tmp = std::env::temp_dir().join("crux-replay-patch.yaml");
               tokio::fs::write(&tmp, patch)
                   .await
                   .map_err(|e| {
                       CruxErr::step_failed(
                           "analysis::replay_dry_run",
                           format!("write temp: {e}"),
                       )
                   })?;

               let output = tokio::process::Command::new("crux")
                   .args(["replay", "--lenient", trace_path,
                          tmp.to_str().unwrap_or("")])
                   .output()
                   .await
                   .map_err(|e| {
                       CruxErr::step_failed(
                           "analysis::replay_dry_run",
                           format!("exec: {e}"),
                       )
                   })?;

               let ok = output.status.success();
               let stderr =
                   String::from_utf8_lossy(&output.stderr).to_string();
               let mismatches: Vec<String> = if ok {
                   vec![]
               } else {
                   stderr.lines().map(|l| l.to_string()).collect()
               };

               Ok(json!({"ok": ok, "mismatches": mismatches}))
           },
       );
   }
   ```

3. Verify:

   ```bash
   cargo nextest run -p crux-agentic -- analysis  -> all green
   cargo clippy -p crux-agentic -- -D warnings    -> zero warnings
   ```

4. Commit: `git commit -m "feat(agentic): add analysis handler module (9 handlers)"`

### Task 2: Create `ci.rs` module

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/ci.rs`
**Run**: `cargo nextest run -p crux-agentic -- ci`

1. Write failing test in `crates/crux-agentic/tests/ci_handlers.rs`:

   ```rust
   use crux_script::HandlerRegistry;
   use serde_json::json;

   fn registry() -> HandlerRegistry {
       let mut r = HandlerRegistry::new();
       crux_agentic::ci::register(&mut r);
       r
   }

   #[tokio::test]
   async fn compile_errors_parses_rustc_output() {
       let reg = registry();
       let h = reg.get_handler("ci::compile_errors").unwrap();
       let input = json!({
           "log": "error[E0308]: mismatched types\n \
                   --> src/main.rs:10:5\n"
       });
       let out = h(input).await.unwrap();
       let errors = out.value["errors"].as_array().unwrap();
       assert_eq!(errors.len(), 1);
       assert_eq!(errors[0]["code"], "E0308");
       assert_eq!(errors[0]["file"], "src/main.rs");
       assert_eq!(errors[0]["line"], 10);
   }

   #[tokio::test]
   async fn clippy_violations_parses_warnings() {
       let reg = registry();
       let h = reg.get_handler("ci::clippy_violations").unwrap();
       let input = json!({
           "log": "warning: unused variable: `x`\n \
                   --> src/lib.rs:5:9\n \
                   = note: `#[warn(unused_variables)]`\n"
       });
       let out = h(input).await.unwrap();
       let violations = out.value["violations"].as_array().unwrap();
       assert_eq!(violations.len(), 1);
       assert_eq!(violations[0]["file"], "src/lib.rs");
   }

   #[tokio::test]
   async fn nextest_failures_parses_test_names() {
       let reg = registry();
       let h = reg.get_handler("ci::nextest_failures").unwrap();
       let input = json!({
           "log": "     FAIL [   0.123s] my-crate::tests::test_foo\n \
                   --- STDOUT: ---\n \
                   thread 'tests::test_foo' panicked at 'assert failed'\n"
       });
       let out = h(input).await.unwrap();
       let failures = out.value["failures"].as_array().unwrap();
       assert_eq!(failures.len(), 1);
       assert!(failures[0]["test_name"].as_str().unwrap()
           .contains("test_foo"));
   }

   #[tokio::test]
   async fn deny_violations_parses_cargo_deny() {
       let reg = registry();
       let h = reg.get_handler("ci::deny_violations").unwrap();
       let input = json!({
           "log": "error[banned]: crate openssl is banned\n \
                   error[license]: crate foo has unapproved license GPL-3.0\n"
       });
       let out = h(input).await.unwrap();
       let violations = out.value["violations"].as_array().unwrap();
       assert_eq!(violations.len(), 2);
   }

   #[tokio::test]
   async fn deduplicate_spans_merges_same_location() {
       let reg = registry();
       let h = reg.get_handler("ci::deduplicate_spans").unwrap();
       let input = json!({
           "errors": [
               {"file": "src/a.rs", "line": 10, "message": "err1"},
               {"file": "src/a.rs", "line": 10, "message": "err2"},
               {"file": "src/b.rs", "line": 5, "message": "err3"},
           ]
       });
       let out = h(input).await.unwrap();
       let deduped = out.value["errors"].as_array().unwrap();
       assert_eq!(deduped.len(), 2);
   }

   #[tokio::test]
   async fn classify_severity_orders_correctly() {
       let reg = registry();
       let h = reg.get_handler("ci::classify_severity").unwrap();
       let input = json!({
           "items": [
               {"source": "clippy", "message": "lint"},
               {"source": "compile", "message": "error"},
               {"source": "test", "message": "fail"},
               {"source": "deny", "message": "banned"},
           ]
       });
       let out = h(input).await.unwrap();
       let ranked = out.value["ranked"].as_array().unwrap();
       assert_eq!(ranked[0]["source"], "compile");
       assert_eq!(ranked[1]["source"], "deny");
   }

   #[tokio::test]
   async fn score_fixability_emits_confidence() {
       let reg = registry();
       let h = reg.get_handler("ci::score_fixability").unwrap();
       let input = json!({
           "ranked": [
               {"source": "clippy", "message": "unused import"},
               {"source": "compile", "message": "missing lifetime"},
           ]
       });
       let out = h(input).await.unwrap();
       assert!(out.confidence.is_some());
   }
   ```

   Run: `cargo nextest run -p crux-agentic -- ci`
   Expected: FAIL

2. Implement `crates/crux-agentic/src/ci.rs`:

   ```rust
   use crux_runtime::prelude::CruxErr;
   use crux_script::{HandlerOutput, HandlerRegistry};
   use serde_json::{Value, json};
   use std::collections::{HashMap, HashSet};

   pub fn register(registry: &mut HandlerRegistry) {
       registry.handler_value(
           "ci::compile_errors",
           |input: Value| async move {
               let log = input
                   .get("log")
                   .and_then(|l| l.as_str())
                   .unwrap_or("");
               let mut errors = Vec::new();
               let lines: Vec<&str> = log.lines().collect();

               for (i, line) in lines.iter().enumerate() {
                   if let Some(rest) =
                       line.trim().strip_prefix("error[")
                   {
                       if let Some(bracket) = rest.find(']') {
                           let code = &rest[..bracket];
                           let message = rest[bracket + 2..].trim();
                           // Next line has location
                           let (file, ln) = if i + 1 < lines.len() {
                               parse_location(lines[i + 1])
                           } else {
                               ("unknown".to_string(), 0)
                           };
                           errors.push(json!({
                               "code": code,
                               "message": message,
                               "file": file,
                               "line": ln
                           }));
                       }
                   }
               }

               Ok(json!({"errors": errors}))
           },
       );

       registry.handler_value(
           "ci::clippy_violations",
           |input: Value| async move {
               let log = input
                   .get("log")
                   .and_then(|l| l.as_str())
                   .unwrap_or("");
               let mut violations = Vec::new();
               let lines: Vec<&str> = log.lines().collect();

               for (i, line) in lines.iter().enumerate() {
                   if line.trim().starts_with("warning:") {
                       let message =
                           line.trim().strip_prefix("warning: ")
                               .unwrap_or("")
                               .to_string();
                       let lint = lines.iter()
                           .skip(i)
                           .take(5)
                           .find_map(|l| {
                               l.find("#[warn(").map(|pos| {
                                   let rest = &l[pos + 7..];
                                   rest.split(')').next()
                                       .unwrap_or("")
                                       .to_string()
                               })
                           })
                           .unwrap_or_default();
                       let (file, ln) = if i + 1 < lines.len() {
                           parse_location(lines[i + 1])
                       } else {
                           ("unknown".to_string(), 0)
                       };
                       violations.push(json!({
                           "lint": lint,
                           "message": message,
                           "file": file,
                           "line": ln
                       }));
                   }
               }

               Ok(json!({"violations": violations}))
           },
       );

       registry.handler_value(
           "ci::nextest_failures",
           |input: Value| async move {
               let log = input
                   .get("log")
                   .and_then(|l| l.as_str())
                   .unwrap_or("");
               let mut failures = Vec::new();

               for line in log.lines() {
                   let trimmed = line.trim();
                   if trimmed.starts_with("FAIL") {
                       // FAIL [   0.123s] crate::module::test_name
                       let test_name = trimmed
                           .split(']')
                           .nth(1)
                           .map(|s| s.trim().to_string())
                           .unwrap_or_default();
                       failures.push(json!({
                           "test_name": test_name,
                           "message": "",
                           "file": ""
                       }));
                   } else if trimmed.starts_with("thread '")
                       && trimmed.contains("panicked at")
                   {
                       // Attach panic message to last failure
                       if let Some(last) = failures.last_mut() {
                           let msg = trimmed
                               .split("panicked at")
                               .nth(1)
                               .map(|s| s.trim().trim_matches('\''))
                               .unwrap_or("");
                           last["message"] =
                               Value::String(msg.to_string());
                       }
                   }
               }

               Ok(json!({"failures": failures}))
           },
       );

       registry.handler_value(
           "ci::deny_violations",
           |input: Value| async move {
               let log = input
                   .get("log")
                   .and_then(|l| l.as_str())
                   .unwrap_or("");
               let mut violations = Vec::new();

               for line in log.lines() {
                   let trimmed = line.trim();
                   if let Some(rest) =
                       trimmed.strip_prefix("error[")
                   {
                       if let Some(bracket) = rest.find(']') {
                           let kind = &rest[..bracket];
                           let message =
                               rest[bracket + 2..].trim().to_string();
                           let crate_name = message
                               .split_whitespace()
                               .nth(1)
                               .unwrap_or("unknown")
                               .to_string();
                           violations.push(json!({
                               "kind": kind,
                               "crate_name": crate_name,
                               "message": message
                           }));
                       }
                   }
               }

               Ok(json!({"violations": violations}))
           },
       );

       registry.handler_value(
           "ci::deduplicate_spans",
           |input: Value| async move {
               let mut result = serde_json::Map::new();

               for key in ["errors", "violations", "failures"] {
                   if let Some(items) =
                       input.get(key).and_then(|v| v.as_array())
                   {
                       let mut seen = HashSet::new();
                       let deduped: Vec<&Value> = items
                           .iter()
                           .filter(|item| {
                               let file = item
                                   .get("file")
                                   .and_then(|f| f.as_str())
                                   .unwrap_or("");
                               let line = item
                                   .get("line")
                                   .and_then(|l| l.as_u64())
                                   .unwrap_or(0);
                               seen.insert(format!("{file}:{line}"))
                           })
                           .collect();
                       result.insert(
                           key.to_string(),
                           Value::Array(
                               deduped
                                   .into_iter()
                                   .cloned()
                                   .collect(),
                           ),
                       );
                   }
               }

               Ok(Value::Object(result))
           },
       );

       registry.handler_value(
           "ci::classify_severity",
           |input: Value| async move {
               let items = input
                   .get("items")
                   .and_then(|i| i.as_array())
                   .cloned()
                   .unwrap_or_default();

               let severity_order = |source: &str| -> u8 {
                   match source {
                       "compile" => 0,
                       "deny" => 1,
                       "test" => 2,
                       "clippy" => 3,
                       _ => 4,
                   }
               };

               let mut ranked: Vec<Value> = items
                   .into_iter()
                   .map(|mut item| {
                       let source = item
                           .get("source")
                           .and_then(|s| s.as_str())
                           .unwrap_or("unknown")
                           .to_string();
                       if let Value::Object(ref mut m) = item {
                           m.insert(
                               "severity".to_string(),
                               Value::String(source.clone()),
                           );
                       }
                       item
                   })
                   .collect();

               ranked.sort_by_key(|item| {
                   let source = item
                       .get("source")
                       .and_then(|s| s.as_str())
                       .unwrap_or("unknown");
                   severity_order(source)
               });

               Ok(json!({"ranked": ranked}))
           },
       );

       registry.handler_value(
           "ci::attach_owners",
           |input: Value| async move {
               let ranked = input
                   .get("ranked")
                   .and_then(|r| r.as_array())
                   .cloned()
                   .unwrap_or_default();

               // Run cargo metadata once to build file->crate map
               let output =
                   tokio::process::Command::new("cargo")
                       .args(["metadata", "--no-deps",
                              "--format-version", "1"])
                       .output()
                       .await;

               let crate_map: HashMap<String, String> =
                   if let Ok(out) = output {
                       let meta: Value = serde_json::from_slice(
                           &out.stdout,
                       )
                       .unwrap_or_default();
                       meta.get("packages")
                           .and_then(|p| p.as_array())
                           .map(|pkgs| {
                               pkgs.iter()
                                   .filter_map(|pkg| {
                                       let name = pkg
                                           .get("name")?
                                           .as_str()?;
                                       let manifest = pkg
                                           .get("manifest_path")?
                                           .as_str()?;
                                       let dir = manifest
                                           .rsplit_once('/')?
                                           .0;
                                       Some((
                                           dir.to_string(),
                                           name.to_string(),
                                       ))
                                   })
                                   .collect()
                           })
                           .unwrap_or_default()
                   } else {
                       HashMap::new()
                   };

               let annotated: Vec<Value> = ranked
                   .into_iter()
                   .map(|mut item| {
                       let file = item
                           .get("file")
                           .and_then(|f| f.as_str())
                           .unwrap_or("");
                       let owner = crate_map
                           .iter()
                           .find(|(dir, _)| file.starts_with(dir.as_str()))
                           .map(|(_, name)| name.as_str())
                           .unwrap_or("unknown");
                       if let Value::Object(ref mut m) = item {
                           m.insert(
                               "crate_name".to_string(),
                               Value::String(owner.to_string()),
                           );
                       }
                       item
                   })
                   .collect();

               Ok(json!({"ranked": annotated}))
           },
       );

       registry.handler(
           "ci::score_fixability",
           |input: Value| async move {
               let ranked = input
                   .get("ranked")
                   .and_then(|r| r.as_array())
                   .cloned()
                   .unwrap_or_default();

               let total = ranked.len() as f64;
               if total == 0.0 {
                   return Ok(HandlerOutput::with_confidence(
                       json!({"ranked": []}),
                       1.0,
                   ));
               }

               let auto_fixable = ranked
                   .iter()
                   .filter(|item| {
                       let source = item
                           .get("source")
                           .and_then(|s| s.as_str())
                           .unwrap_or("");
                       let msg = item
                           .get("message")
                           .and_then(|m| m.as_str())
                           .unwrap_or("");
                       source == "clippy"
                           || msg.contains("unused import")
                           || msg.contains("unused variable")
                   })
                   .count() as f64;

               let score = auto_fixable / total;

               Ok(HandlerOutput::with_confidence(
                   json!({"ranked": ranked}),
                   score as f32,
               ))
           },
       );
   }

   fn parse_location(line: &str) -> (String, u64) {
       // Parse " --> src/main.rs:10:5"
       let trimmed = line.trim().strip_prefix("-->").unwrap_or(line);
       let trimmed = trimmed.trim();
       if let Some((file, rest)) = trimmed.rsplit_once(':') {
           if let Some((file, line_str)) = file.rsplit_once(':') {
               if let Ok(ln) = line_str.parse::<u64>() {
                   return (file.to_string(), ln);
               }
           }
       }
       (trimmed.to_string(), 0)
   }
   ```

3. Verify:

   ```bash
   cargo nextest run -p crux-agentic -- ci  -> all green
   cargo clippy -p crux-agentic -- -D warnings  -> zero warnings
   ```

4. Commit: `git commit -m "feat(agentic): add ci handler module (8 handlers)"`

### Task 3: Create `review.rs` module

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/review.rs`
**Run**: `cargo nextest run -p crux-agentic -- review`

1. Write failing test in `crates/crux-agentic/tests/review.rs`:

   ```rust
   use crux_script::HandlerRegistry;
   use serde_json::json;

   fn registry() -> HandlerRegistry {
       let mut r = HandlerRegistry::new();
       crux_agentic::review::register(&mut r);
       r
   }

   #[tokio::test]
   async fn normalize_findings_merges_sources() {
       let reg = registry();
       let h = reg.get_handler("review::normalize_findings").unwrap();
       let input = json!({
           "clippy": {"violations": [
               {"lint": "unused", "file": "a.rs", "line": 1,
                "message": "unused var"}
           ]},
           "arch": {"violations": [
               {"file": "b.rs", "imports": "infra::db",
                "violation": "domain imports infra"}
           ]},
           "coverage": {"uncovered": ["c.rs:10"]}
       });
       let out = h(input).await.unwrap();
       let findings = out.value["findings"].as_array().unwrap();
       assert!(findings.len() >= 3);
       assert!(findings.iter().all(|f| f.get("source").is_some()));
   }

   #[tokio::test]
   async fn apply_severity_tiers_findings() {
       let reg = registry();
       let h = reg.get_handler("review::apply_severity").unwrap();
       let input = json!({
           "findings": [
               {"source": "compile", "message": "error"},
               {"source": "clippy", "message": "suggestion"},
               {"source": "coverage", "message": "uncovered"},
           ]
       });
       let out = h(input).await.unwrap();
       let findings = out.value["findings"].as_array().unwrap();
       assert_eq!(findings[0]["tier"], "blocking");
       assert_eq!(findings[1]["tier"], "suggestion");
       assert_eq!(findings[2]["tier"], "observation");
   }

   #[tokio::test]
   async fn compute_score_emits_confidence() {
       let reg = registry();
       let h = reg.get_handler("review::compute_score").unwrap();
       let input = json!({
           "findings": [
               {"tier": "blocking", "file": "a.rs"},
               {"tier": "suggestion", "file": "b.rs"},
               {"tier": "observation", "file": "c.rs"},
           ]
       });
       let out = h(input).await.unwrap();
       assert!(out.confidence.is_some());
       // 1 blocking out of 3 -> score should reflect that
       let score = out.confidence.unwrap();
       assert!(score < 1.0);
   }

   #[tokio::test]
   async fn compute_score_perfect_when_no_blocking() {
       let reg = registry();
       let h = reg.get_handler("review::compute_score").unwrap();
       let input = json!({
           "findings": [
               {"tier": "suggestion"},
               {"tier": "observation"},
           ]
       });
       let out = h(input).await.unwrap();
       assert_eq!(out.confidence.unwrap(), 1.0);
   }
   ```

   Run: `cargo nextest run -p crux-agentic -- review`
   Expected: FAIL

2. Implement `crates/crux-agentic/src/review.rs`:

   ```rust
   use crux_runtime::prelude::CruxErr;
   use crux_script::{HandlerOutput, HandlerRegistry};
   use serde_json::{Value, json};

   pub fn register(registry: &mut HandlerRegistry) {
       registry.handler_value(
           "review::arch_boundary_check",
           |input: Value| async move {
               let files = input
                   .get("files")
                   .and_then(|f| f.as_array())
                   .cloned()
                   .unwrap_or_default();

               let file_list: Vec<&str> = files
                   .iter()
                   .filter_map(|f| f.as_str())
                   .collect();

               if file_list.is_empty() {
                   return Ok(json!({"violations": []}));
               }

               // Search for adapter/infra imports in domain crate files
               let pattern =
                   r"use\s+(crate::adapters|infra::|adapter::)";
               let mut violations = Vec::new();

               for file in &file_list {
                   let output =
                       tokio::process::Command::new("rg")
                           .args([
                               "--no-heading", "-n", pattern, file,
                           ])
                           .output()
                           .await;

                   if let Ok(out) = output {
                       if out.status.success() {
                           let stdout = String::from_utf8_lossy(
                               &out.stdout,
                           );
                           for line in stdout.lines() {
                               violations.push(json!({
                                   "file": file,
                                   "imports": line.trim(),
                                   "violation":
                                       "domain imports adapter/infra"
                               }));
                           }
                       }
                   }
               }

               Ok(json!({"violations": violations}))
           },
       );

       registry.handler_value(
           "review::normalize_findings",
           |input: Value| async move {
               let mut findings = Vec::new();

               // Clippy violations
               if let Some(violations) = input
                   .pointer("/clippy/violations")
                   .and_then(|v| v.as_array())
               {
                   for v in violations {
                       findings.push(json!({
                           "source": "clippy",
                           "file": v.get("file")
                               .unwrap_or(&Value::Null),
                           "line": v.get("line")
                               .unwrap_or(&Value::Null),
                           "message": v.get("message")
                               .or_else(|| v.get("lint"))
                               .unwrap_or(&Value::Null),
                           "severity": "warning"
                       }));
                   }
               }

               // Arch violations
               if let Some(violations) = input
                   .pointer("/arch/violations")
                   .and_then(|v| v.as_array())
               {
                   for v in violations {
                       findings.push(json!({
                           "source": "arch",
                           "file": v.get("file")
                               .unwrap_or(&Value::Null),
                           "line": null,
                           "message": v.get("violation")
                               .unwrap_or(&Value::Null),
                           "severity": "error"
                       }));
                   }
               }

               // Coverage gaps
               if let Some(uncovered) = input
                   .pointer("/coverage/uncovered")
                   .and_then(|v| v.as_array())
               {
                   for u in uncovered {
                       let loc = u.as_str().unwrap_or("");
                       let (file, line) =
                           loc.rsplit_once(':').unwrap_or((loc, "0"));
                       findings.push(json!({
                           "source": "coverage",
                           "file": file,
                           "line": line.parse::<u64>().unwrap_or(0),
                           "message": "uncovered code path",
                           "severity": "info"
                       }));
                   }
               }

               Ok(json!({"findings": findings}))
           },
       );

       registry.handler_value(
           "review::apply_severity",
           |input: Value| async move {
               let findings = input
                   .get("findings")
                   .and_then(|f| f.as_array())
                   .cloned()
                   .unwrap_or_default();

               let tiered: Vec<Value> = findings
                   .into_iter()
                   .map(|mut f| {
                       let source = f
                           .get("source")
                           .and_then(|s| s.as_str())
                           .unwrap_or("");
                       let tier = match source {
                           "compile" | "arch" | "deny" => "blocking",
                           "clippy" | "test" => "suggestion",
                           _ => "observation",
                       };
                       if let Value::Object(ref mut m) = f {
                           m.insert(
                               "tier".to_string(),
                               Value::String(tier.to_string()),
                           );
                       }
                       f
                   })
                   .collect();

               Ok(json!({"findings": tiered}))
           },
       );

       registry.handler(
           "review::compute_score",
           |input: Value| async move {
               let findings = input
                   .get("findings")
                   .and_then(|f| f.as_array())
                   .cloned()
                   .unwrap_or_default();

               let total = findings.len() as f64;
               if total == 0.0 {
                   return Ok(HandlerOutput::with_confidence(
                       json!({"score": 1.0, "blocking_count": 0}),
                       1.0,
                   ));
               }

               let blocking = findings
                   .iter()
                   .filter(|f| {
                       f.get("tier")
                           .and_then(|t| t.as_str())
                           == Some("blocking")
                   })
                   .count() as f64;

               let score = 1.0 - (blocking / total);

               Ok(HandlerOutput::with_confidence(
                   json!({
                       "score": score,
                       "blocking_count": blocking as u64,
                       "total_findings": total as u64
                   }),
                   score as f32,
               ))
           },
       );

       registry.handler_value(
           "review::approve",
           |input: Value| async move {
               let pr_number = input
                   .get("args")
                   .and_then(|a| a.get("pr"))
                   .and_then(|p| p.as_str())
                   .or_else(|| {
                       input.get("pr").and_then(|p| p.as_str())
                   });

               let mut cmd =
                   tokio::process::Command::new("gh");
               cmd.args(["pr", "review", "--approve"]);
               if let Some(pr) = pr_number {
                   cmd.arg(pr);
               }

               let output = cmd.output().await.map_err(|e| {
                   CruxErr::step_failed(
                       "review::approve",
                       format!("exec: {e}"),
                   )
               })?;

               if output.status.success() {
                   Ok(json!({"approved": true}))
               } else {
                   let stderr = String::from_utf8_lossy(
                       &output.stderr,
                   );
                   Err(CruxErr::step_failed(
                       "review::approve",
                       format!("gh pr review failed: {stderr}"),
                   ))
               }
           },
       );
   }
   ```

3. Verify:

   ```bash
   cargo nextest run -p crux-agentic -- review  -> all green
   cargo clippy -p crux-agentic -- -D warnings  -> zero warnings
   ```

4. Commit: `git commit -m "feat(agentic): add review handler module (5 handlers)"`

### Task 4: Create `triage.rs` module

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/triage.rs`
**Run**: `cargo nextest run -p crux-agentic -- triage`

1. Write failing test in `crates/crux-agentic/tests/triage.rs`:

   ```rust
   use crux_script::HandlerRegistry;
   use serde_json::json;

   fn registry() -> HandlerRegistry {
       let mut r = HandlerRegistry::new();
       crux_agentic::triage::register(&mut r);
       r
   }

   #[tokio::test]
   async fn parse_repo_tags_extracts_repo() {
       let reg = registry();
       let h = reg.get_handler("triage::parse_repo_tags").unwrap();
       let input = json!({
           "todos": [
               {"id": "1", "title": "fix bug",
                "metadata": {"repo": "crux"}},
               {"id": "2", "title": "add test",
                "metadata": {"repo": "minibox"}},
           ]
       });
       let out = h(input).await.unwrap();
       let todos = out.value["todos"].as_array().unwrap();
       assert_eq!(todos[0]["repo"], "crux");
       assert_eq!(todos[1]["repo"], "minibox");
   }

   #[tokio::test]
   async fn score_urgency_sorts_by_score() {
       let reg = registry();
       let h = reg.get_handler("triage::score_urgency").unwrap();
       let input = json!({
           "todos": [
               {"id": "1", "priority": "low",
                "created_at": "2026-01-01T00:00:00Z"},
               {"id": "2", "priority": "high",
                "created_at": "2026-05-01T00:00:00Z"},
               {"id": "3", "priority": "high",
                "created_at": "2026-01-01T00:00:00Z"},
           ]
       });
       let out = h(input).await.unwrap();
       let todos = out.value["todos"].as_array().unwrap();
       // Oldest high-priority first
       assert_eq!(todos[0]["id"], "3");
   }

   #[tokio::test]
   async fn deduplicate_intent_clusters_similar() {
       let reg = registry();
       let h = reg.get_handler("triage::deduplicate_intent").unwrap();
       let input = json!({
           "todos": [
               {"id": "1", "title": "fix login bug"},
               {"id": "2", "title": "fix login bug in auth"},
               {"id": "3", "title": "add dark mode"},
           ]
       });
       let out = h(input).await.unwrap();
       let groups = out.value["groups"].as_array().unwrap();
       // "fix login bug" and "fix login bug in auth" should cluster
       assert!(groups.len() <= 2);
   }

   #[tokio::test]
   async fn group_by_repo_partitions() {
       let reg = registry();
       let h = reg.get_handler("triage::group_by_repo").unwrap();
       let input = json!({
           "todos": [
               {"id": "1", "repo": "crux"},
               {"id": "2", "repo": "minibox"},
               {"id": "3", "repo": "crux"},
           ]
       });
       let out = h(input).await.unwrap();
       let repos = out.value["repos"].as_object().unwrap();
       assert_eq!(repos["crux"].as_array().unwrap().len(), 2);
       assert_eq!(repos["minibox"].as_array().unwrap().len(), 1);
   }
   ```

   Run: `cargo nextest run -p crux-agentic -- triage`
   Expected: FAIL

2. Implement `crates/crux-agentic/src/triage.rs`:

   ```rust
   use chrono::{DateTime, Utc};
   use crux_script::HandlerRegistry;
   use serde_json::{Value, json};
   use std::collections::HashMap;

   pub fn register(registry: &mut HandlerRegistry) {
       registry.handler_value(
           "triage::parse_repo_tags",
           |input: Value| async move {
               let todos = input
                   .get("todos")
                   .and_then(|t| t.as_array())
                   .cloned()
                   .unwrap_or_default();

               let tagged: Vec<Value> = todos
                   .into_iter()
                   .map(|mut todo| {
                       let repo = todo
                           .pointer("/metadata/repo")
                           .and_then(|r| r.as_str())
                           .unwrap_or("unknown")
                           .to_string();
                       if let Value::Object(ref mut m) = todo {
                           m.insert(
                               "repo".to_string(),
                               Value::String(repo),
                           );
                       }
                       todo
                   })
                   .collect();

               Ok(json!({"todos": tagged}))
           },
       );

       registry.handler_value(
           "triage::score_urgency",
           |input: Value| async move {
               let todos = input
                   .get("todos")
                   .and_then(|t| t.as_array())
                   .cloned()
                   .unwrap_or_default();

               let now = Utc::now();
               let mut scored: Vec<(f64, Value)> = todos
                   .into_iter()
                   .map(|mut todo| {
                       let priority = todo
                           .get("priority")
                           .and_then(|p| p.as_str())
                           .unwrap_or("medium");
                       let weight: f64 = match priority {
                           "critical" => 4.0,
                           "high" => 3.0,
                           "medium" => 2.0,
                           "low" => 1.0,
                           _ => 1.0,
                       };
                       let created = todo
                           .get("created_at")
                           .and_then(|c| c.as_str())
                           .and_then(|s| {
                               s.parse::<DateTime<Utc>>().ok()
                           });
                       let age_days = created
                           .map(|c| (now - c).num_days() as f64)
                           .unwrap_or(0.0);
                       let score = age_days * weight;

                       if let Value::Object(ref mut m) = todo {
                           m.insert(
                               "urgency_score".to_string(),
                               json!(score),
                           );
                       }
                       (score, todo)
                   })
                   .collect();

               scored.sort_by(|a, b| {
                   b.0.partial_cmp(&a.0).unwrap_or(
                       std::cmp::Ordering::Equal,
                   )
               });

               let result: Vec<Value> =
                   scored.into_iter().map(|(_, t)| t).collect();
               Ok(json!({"todos": result}))
           },
       );

       registry.handler_value(
           "triage::deduplicate_intent",
           |input: Value| async move {
               let todos = input
                   .get("todos")
                   .and_then(|t| t.as_array())
                   .cloned()
                   .unwrap_or_default();

               let mut groups: Vec<(String, Vec<Value>)> = Vec::new();

               for todo in todos {
                   let title = todo
                       .get("title")
                       .and_then(|t| t.as_str())
                       .unwrap_or("")
                       .to_lowercase();

                   let matched = groups.iter_mut().find(|(canonical, _)| {
                       let dist = edit_distance(canonical, &title);
                       let max_len = canonical
                           .len()
                           .max(title.len())
                           as f64;
                       if max_len == 0.0 {
                           return true;
                       }
                       (dist as f64 / max_len) < 0.4
                   });

                   if let Some((_, members)) = matched {
                       members.push(todo);
                   } else {
                       groups.push((title, vec![todo]));
                   }
               }

               let result: Vec<Value> = groups
                   .into_iter()
                   .map(|(canonical, members)| {
                       let ids: Vec<Value> = members
                           .iter()
                           .filter_map(|m| m.get("id").cloned())
                           .collect();
                       json!({
                           "canonical": canonical,
                           "duplicates": ids,
                           "items": members
                       })
                   })
                   .collect();

               Ok(json!({"groups": result}))
           },
       );

       registry.handler_value(
           "triage::group_by_repo",
           |input: Value| async move {
               let todos = input
                   .get("todos")
                   .and_then(|t| t.as_array())
                   .cloned()
                   .unwrap_or_default();

               let mut repos: HashMap<String, Vec<Value>> =
                   HashMap::new();
               for todo in todos {
                   let repo = todo
                       .get("repo")
                       .and_then(|r| r.as_str())
                       .unwrap_or("unknown")
                       .to_string();
                   repos.entry(repo).or_default().push(todo);
               }

               let map: serde_json::Map<String, Value> = repos
                   .into_iter()
                   .map(|(k, v)| (k, Value::Array(v)))
                   .collect();

               Ok(json!({"repos": map}))
           },
       );
   }

   /// Simple Levenshtein edit distance.
   fn edit_distance(a: &str, b: &str) -> usize {
       let a: Vec<char> = a.chars().collect();
       let b: Vec<char> = b.chars().collect();
       let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];

       for i in 0..=a.len() {
           dp[i][0] = i;
       }
       for j in 0..=b.len() {
           dp[0][j] = j;
       }
       for i in 1..=a.len() {
           for j in 1..=b.len() {
               let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
               dp[i][j] = (dp[i - 1][j] + 1)
                   .min(dp[i][j - 1] + 1)
                   .min(dp[i - 1][j - 1] + cost);
           }
       }
       dp[a.len()][b.len()]
   }
   ```

3. Verify:

   ```bash
   cargo nextest run -p crux-agentic -- triage  -> all green
   cargo clippy -p crux-agentic -- -D warnings  -> zero warnings
   ```

4. Commit: `git commit -m "feat(agentic): add triage handler module (4 handlers)"`

### Task 5: Wire modules into `lib.rs` and `register_all`

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/lib.rs`, `crates/crux-agentic/src/handlers.rs`
**Run**: `cargo nextest run -p crux-agentic`

1. Add to `lib.rs` after `pub mod sqlite;`:

   ```rust
   pub mod analysis;
   pub mod ci;
   pub mod review;
   pub mod triage;
   ```

2. Add to `register_all_with_plugins()` body:

   ```rust
   analysis::register(registry);
   ci::register(registry);
   review::register(registry);
   triage::register(registry);
   ```

3. Add constants to `handlers.rs`:

   ```rust
   // analysis
   pub const ANALYSIS_LATENCY_PROFILE: &str = "analysis::latency_profile";
   pub const ANALYSIS_TOKEN_SPEND: &str = "analysis::token_spend";
   pub const ANALYSIS_FAILURE_CLUSTERS: &str = "analysis::failure_clusters";
   pub const ANALYSIS_REPLAY_CACHE_HITS: &str = "analysis::replay_cache_hits";
   pub const ANALYSIS_TIGHTEN_BUDGET: &str = "analysis::tighten_budget";
   pub const ANALYSIS_COMPRESS_STAGES: &str = "analysis::compress_stages";
   pub const ANALYSIS_TUNE_RETRY: &str = "analysis::tune_retry";
   pub const ANALYSIS_PATCH_SCHEMA_CHECK: &str = "analysis::patch_schema_check";
   pub const ANALYSIS_REPLAY_DRY_RUN: &str = "analysis::replay_dry_run";

   // ci
   pub const CI_COMPILE_ERRORS: &str = "ci::compile_errors";
   pub const CI_CLIPPY_VIOLATIONS: &str = "ci::clippy_violations";
   pub const CI_NEXTEST_FAILURES: &str = "ci::nextest_failures";
   pub const CI_DENY_VIOLATIONS: &str = "ci::deny_violations";
   pub const CI_DEDUPLICATE_SPANS: &str = "ci::deduplicate_spans";
   pub const CI_CLASSIFY_SEVERITY: &str = "ci::classify_severity";
   pub const CI_ATTACH_OWNERS: &str = "ci::attach_owners";
   pub const CI_SCORE_FIXABILITY: &str = "ci::score_fixability";

   // review
   pub const REVIEW_ARCH_BOUNDARY_CHECK: &str = "review::arch_boundary_check";
   pub const REVIEW_NORMALIZE_FINDINGS: &str = "review::normalize_findings";
   pub const REVIEW_APPLY_SEVERITY: &str = "review::apply_severity";
   pub const REVIEW_COMPUTE_SCORE: &str = "review::compute_score";
   pub const REVIEW_APPROVE: &str = "review::approve";

   // triage
   pub const TRIAGE_PARSE_REPO_TAGS: &str = "triage::parse_repo_tags";
   pub const TRIAGE_SCORE_URGENCY: &str = "triage::score_urgency";
   pub const TRIAGE_DEDUPLICATE_INTENT: &str = "triage::deduplicate_intent";
   pub const TRIAGE_GROUP_BY_REPO: &str = "triage::group_by_repo";
   ```

4. Update `tests/register_all.rs` — add all new handler names to `expected` array.

5. Verify:

   ```bash
   cargo nextest run -p crux-agentic  -> all green
   cargo clippy -p crux-agentic -- -D warnings  -> zero warnings
   ```

6. Commit: `git commit -m "feat(agentic): wire analysis/ci/review/triage into register_all"`

### Task 6: Check `serde_yaml` and `chrono` dependencies

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/Cargo.toml`
**Run**: `cargo check -p crux-agentic`

1. Verify `chrono` and `serde_yaml` are in `Cargo.toml`. If missing, add:

   ```toml
   chrono = { version = "0.4", features = ["serde"] }
   serde_yaml = "0.9"
   ```

2. Run: `cargo check -p crux-agentic`

3. Commit (if changed):
   `git commit -m "chore(agentic): add chrono and serde_yaml deps"`

### Task 7: Update pipeline files

**File(s)**: All 5 pipeline files in `examples/joe/`

1. `examples/joe/agent_meta_eval.crux` — replace 9 noops:
   - `latency_profile` -> `analysis::latency_profile`
   - `token_spend_breakdown` -> `analysis::token_spend`
   - `step_failure_clustering` -> `analysis::failure_clusters`
   - `replay_cache_analysis` -> `analysis::replay_cache_hits`
   - `tighten_budget` -> `analysis::tighten_budget`
   - `compress_prompt_stages` -> `analysis::compress_stages`
   - `tune_retry_policy` -> `analysis::tune_retry`
   - `patch_schema_check` -> `analysis::patch_schema_check`
   - `replay_dry_run` -> `analysis::replay_dry_run`

2. `examples/joe/ci_triage.yaml` — replace 8 noops:
   - `compile_errors` -> `ci::compile_errors`
   - `clippy_violations` -> `ci::clippy_violations`
   - `nextest_failures` -> `ci::nextest_failures`
   - `deny_violations` -> `ci::deny_violations`
   - `deduplicate_spans` -> `ci::deduplicate_spans`
   - `classify_severity` -> `ci::classify_severity`
   - `attach_owners` -> `ci::attach_owners`
   - `score_fixability` -> `ci::score_fixability`

3. `examples/joe/crate_refactor.yaml` — replace 6 noops:
   - `dep_graph_analysis` -> `review::arch_boundary_check`
   - `arch_boundary_check` -> `review::arch_boundary_check`
   - `extract_trait_port` -> `llm::invoke` with `args.prompt` for trait extraction
   - `split_crate` -> `llm::invoke` with `args.prompt` for crate scaffold
   - `add_adapter_layer` -> `llm::invoke` with `args.prompt` for adapter scaffold
   - `generate_patch` -> `llm::invoke` with `args.prompt` for diff generation

4. `examples/joe/pr_review.crux` — replace 5 noops:
   - `arch_boundary_check` -> `review::arch_boundary_check`
   - `normalize_findings` -> `review::normalize_findings`
   - `apply_severity_matrix` -> `review::apply_severity`
   - `compute_review_score` -> `review::compute_score`
   - `approve` -> `review::approve`

5. `examples/joe/doob_triage.yaml` — replace 4 noops:
   - `parse_repo_tags` -> `triage::parse_repo_tags`
   - `score_urgency` -> `triage::score_urgency`
   - `deduplicate_intent` -> `triage::deduplicate_intent`
   - `group_by_repo` -> `triage::group_by_repo`

6. Remove `ASPIRATIONAL TEMPLATE` headers from all 5 files.

7. Commit:
   `git commit -m "feat(pipelines): replace noop placeholders with real handlers"`

### Task 8: Update capabilities doc

**File(s)**: `docs/crux-capabilities.md`

1. Add 4 new sections to "Native Handlers" table:
   - analysis (9 handlers)
   - ci (8 handlers)
   - review (5 handlers)
   - triage (4 handlers)

2. Remove from "Known Gaps":
   - The entire "Domain analysis arms" entry
   - `ctrl::noop` references

3. Update handler count in prose if present.

4. Commit:
   `git commit -m "docs: update capabilities with 26 new handlers"`

## Pre-Save Checklist

- [x] Every noop placeholder maps to at least one task
- [x] No placeholders or vague directives
- [x] Method names consistent across tasks and pipeline files
- [x] Each task is focused (one module or one concern)
- [x] Each task ends with a commit
- [x] Dependencies noted (Task 6 before Tasks 1-4 if deps missing)
