# Plan: Reliability + DX Improvement Cohort

## Goal

Make pipeline features trustworthy (cohort A: #64, #67, #68, #82) and crux
traces debuggable (cohort C: #76, #79, #83).

## Architecture

- Crates affected: `crux-script`, `crux-runtime`, `crux-domain`, `crux-types`
- New types: `CitedFinding` (crux-types), `StepEvent::Intermediate` variant
  (crux-domain)
- New methods: `Runner::run_validated()`, `CruxCtx::emit_step_event()`,
  `CruxCtx::cite()`, `Crux::to_trace_json()`, `Crux::to_mermaid()`
- Data flow unchanged -- all additions are layered on existing infrastructure

## Tech Stack

- Rust 2024, MSRV 1.88
- No new dependencies. Mermaid/JSON export is pure string formatting.
- `tokio::sync::broadcast` already in use for `EventPipeline`

## Tasks

### Task 1: Duplicate step name detection in validator (#82a)

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/validator.rs`
**Run**: `cargo nextest run -p crux-script`

1. Write failing test:

   ```rust
   #[test]
   fn duplicate_step_names_produce_error() {
       let yaml = r#"
   pipeline: dup-test
   steps:
     - step: read
       handler: ctrl::noop
     - step: read
       handler: ctrl::noop
   "#;
       let pipeline = crate::load(yaml).unwrap();
       let mut reg = crate::registry::HandlerRegistry::new();
       register_ctrl_noop(&mut reg);
       let report = validate_pipeline(&pipeline, &reg);
       assert!(report.diagnostics.iter().any(|d| {
           d.severity == DiagnosticSeverity::Error
               && d.message.contains("duplicate step name")
       }));
   }
   ```

   Run: `cargo nextest run -p crux-script -- duplicate_step`
   Expected: FAIL (no duplicate detection yet)

2. Implement in `validate_pipeline()` -- add a `HashSet<String>` before
   the step loop, collect step names, emit error on collision:

   ```rust
   // At the top of validate_pipeline, before the for loop:
   let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

   // Inside the for loop, after determining the step name for each variant:
   // (extract step name from each StepDef variant)
   let step_name = match step {
       StepDef::Step(n) => n.step.as_str(),
       StepDef::Delegate(n) => n.name.as_deref().unwrap_or(&n.delegate),
       StepDef::Pipe(n) => n.pipe.as_str(),
       StepDef::JoinAll(n) => n.join_all.as_str(),
       StepDef::RouteOnConfidence(n) => n.route_on_confidence.as_str(),
       StepDef::Speculate(n) => n.speculate.as_str(),
   };
   if !seen_names.insert(step_name.to_string()) {
       report.push(ValidationDiagnostic::error(
           &location,
           format!("duplicate step name '{step_name}'"),
       ));
   }
   ```

3. Verify:

   ```text
   cargo nextest run -p crux-script    -> all green
   cargo clippy -p crux-script -- -D warnings  -> zero warnings
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-script): detect duplicate step names in validator (#82)"`

### Task 2: Wire validation into Runner::run (#64)

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/runner.rs`
**Run**: `cargo nextest run -p crux-script`

1. Write failing test:

   ```rust
   #[tokio::test]
   async fn run_rejects_pipeline_with_validation_errors() {
       let yaml = r#"
   pipeline: bad
   steps:
     - step: s1
       handler: shell::nonexistent_in_known_ns
   "#;
       let pipeline = crate::load(yaml).unwrap();
       let mut reg = HandlerRegistry::new();
       // Register one shell:: handler so the namespace is known.
       reg.handler_value("shell::exec", |v: Value| async { Ok(v) });
       let runner = Runner::new(Arc::new(reg));
       let crux = runner.run(&pipeline, serde_json::json!({})).await;
       assert!(crux.value().is_err());
       let err = crux.value().unwrap_err();
       assert!(err.to_string().contains("validation"));
   }
   ```

   Run: `cargo nextest run -p crux-script -- run_rejects_pipeline`
   Expected: FAIL (Runner::run doesn't validate)

2. Implement -- add validation to `run_inner()`:

   ```rust
   // At the top of run_inner, before creating CruxCtx:
   let report = crate::validator::validate_pipeline(pipeline, &self.registry);
   if !report.is_ok() {
       let errors: Vec<String> = report
           .diagnostics
           .iter()
           .filter(|d| d.severity == crate::validator::DiagnosticSeverity::Error)
           .map(|d| format!("{}: {}", d.location, d.message))
           .collect();
       let mut ctx = CruxCtx::new(&pipeline.pipeline);
       let err = CruxErr::step_failed(
           &pipeline.pipeline,
           format!("pipeline validation failed:\n{}", errors.join("\n")),
       );
       return ctx.finalize(Err(err));
   }
   for diag in &report.diagnostics {
       if diag.severity == crate::validator::DiagnosticSeverity::Warning {
           eprintln!("[crux] warning: {}: {}", diag.location, diag.message);
       }
   }
   ```

3. Add `Runner::run_unchecked()` that delegates to `run_inner()` without
   validation (rename current `run_inner` to `run_core`, have both
   `run_inner` and `run_unchecked` call it):

   ```rust
   /// Run without pre-flight validation. Use for tests or REPL.
   pub async fn run_unchecked(
       &self,
       pipeline: &PipelineDef,
       input: Value,
   ) -> Crux<Value> {
       self.run_core(pipeline, input, None, ReplayMode::Strict).await
   }
   ```

4. Verify:

   ```text
   cargo nextest run -p crux-script    -> all green
   cargo clippy -p crux-script -- -D warnings  -> zero warnings
   ```

5. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-script): validate pipeline before execution (#64)"`

### Task 3: Speculation score fallback warning (#68)

**Crate**: `crux-runtime`
**File(s)**: `crates/crux-runtime/src/speculation.rs`
**Run**: `cargo nextest run -p crux-runtime`

1. Write failing test:

   ```rust
   #[tokio::test]
   async fn pick_best_warns_on_missing_score() {
       // This test verifies the fallback path is exercised.
       // The actual warning is logged to stderr -- we test that the
       // byte-length fallback still produces a valid winner.
       let mut ctx = CruxCtx::new("test");
       let arms = vec![
           ("short", Box::pin(async { Ok::<Value, CruxErr>(json!({"text": "a"})) })
               as Pin<Box<dyn Future<Output = Result<Value, CruxErr>> + Send>>),
           ("long", Box::pin(async { Ok::<Value, CruxErr>(json!({"text": "longer content wins"})) })
               as Pin<Box<dyn Future<Output = Result<Value, CruxErr>> + Send>>),
       ];
       let result = ctx.speculate("test-spec", arms).pick_best().await.unwrap();
       // Longer content should win via byte-length fallback
       assert!(result.get("text").unwrap().as_str().unwrap().len() > 5);
   }
   ```

   Run: `cargo nextest run -p crux-runtime -- pick_best_warns`
   Expected: PASS (behavior exists, this is a characterization test)

2. Add `eprintln!` warning in `pick_best()` when no arm has a `score`
   field:

   ```rust
   // In SpeculationBuilder::pick_best, after collecting results:
   pub async fn pick_best(self) -> Result<T, CruxErr> {
       let name = self.name.clone();
       self.pick_best_by(|val| {
           let json = serde_json::to_value(val).unwrap_or(serde_json::Value::Null);
           if let Some(score) = json.get("score").and_then(|v| v.as_f64()) {
               return score as f32;
           }
           eprintln!(
               "[crux] warning: speculate '{}' arm has no 'score' field, \
                falling back to output length",
               name
           );
           json.to_string().len() as f32
       })
       .await
   }
   ```

3. Update the TODO comment to note the warning is now emitted.

4. Verify:

   ```text
   cargo nextest run -p crux-runtime    -> all green
   cargo clippy -p crux-runtime -- -D warnings  -> zero warnings
   ```

5. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-runtime): warn on speculation score fallback (#68)"`

### Task 4: Clarify delegate TODO + add ctrl::echo agent (#67)

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/runner.rs`, `crates/crux-stdlib/src/lib.rs`
**Run**: `cargo nextest run -p crux-script`

1. Write failing test:

   ```rust
   #[tokio::test]
   async fn delegate_to_echo_agent_returns_input() {
       let yaml = r#"
   pipeline: delegate-test
   steps:
     - delegate: echo
       name: echo-step
   "#;
       let pipeline = crate::load(yaml).unwrap();
       let mut reg = HandlerRegistry::new();
       crux_stdlib::ctrl::register_echo_agent(&mut reg);
       let runner = Runner::new(Arc::new(reg));
       let input = serde_json::json!({"msg": "hello"});
       let crux = runner.run_unchecked(&pipeline, input.clone()).await;
       assert_eq!(*crux.value().unwrap(), input);
   }
   ```

   Run: `cargo nextest run -p crux-script -- delegate_to_echo`
   Expected: FAIL (no echo agent registered)

2. Implement `ctrl::echo` agent in crux-stdlib:

   ```rust
   // In crates/crux-stdlib/src/ctrl.rs (or new file):
   /// Register a `ctrl::echo` agent that returns its input unchanged.
   /// Useful for testing delegation pipelines.
   pub fn register_echo_agent(registry: &mut crux_script::HandlerRegistry) {
       registry.agent_fn("echo", |input: serde_json::Value| async move { Ok(input) });
   }
   ```

3. Update the TODO comment in `runner.rs`:

   ```rust
   // delegate: works when agents are registered via registry.agent() or
   // registry.agent_fn(). No built-in agents are registered by default —
   // use crux_stdlib::ctrl::register_echo_agent() for a test/debug primitive.
   ```

4. Verify:

   ```text
   cargo nextest run -p crux-script    -> all green
   cargo clippy -p crux-script -- -D warnings  -> zero warnings
   ```

5. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-stdlib): add ctrl::echo agent for delegation testing (#67)"`

### Task 5: Add step_name to StepEvent::Chunk (#76)

**Crate**: `crux-domain`
**File(s)**: `crates/crux-domain/src/event.rs`
**Run**: `cargo nextest run -p crux-domain`

1. Write failing test:

   ```rust
   #[test]
   fn chunk_event_has_step_name() {
       let event = StepEvent::Chunk {
           step_name: "my-step".to_string(),
           payload: serde_json::json!({"delta": "hello"}),
       };
       let json = serde_json::to_value(&event).unwrap();
       assert_eq!(json["step_name"], "my-step");
       assert_eq!(json["kind"], "chunk");
   }
   ```

   Run: `cargo nextest run -p crux-domain -- chunk_event_has_step_name`
   Expected: FAIL (Chunk has no step_name field)

2. Add `step_name: String` to `StepEvent::Chunk`:

   ```rust
   /// An intermediate streaming chunk from a streaming step.
   Chunk {
       step_name: String,
       payload: serde_json::Value,
   },
   ```

3. Fix all existing references to `StepEvent::Chunk` (grep for them).

4. Verify:

   ```text
   cargo nextest run -p crux-domain    -> all green
   cargo clippy -p crux-domain -- -D warnings  -> zero warnings
   ```

5. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-domain): add step_name to StepEvent::Chunk (#76)"`

### Task 6: Add CruxCtx::emit_step_event() (#76)

**Crate**: `crux-runtime`
**File(s)**: `crates/crux-runtime/src/ctx.rs`
**Run**: `cargo nextest run -p crux-runtime`

1. Write failing test in `crates/crux-runtime/src/event_sink.rs`:

   ```rust
   #[tokio::test]
   async fn emit_step_event_sends_chunk() {
       let pipeline = EventPipeline::new(64);
       let mut rx = pipeline.subscribe();

       let mut ctx = CruxCtx::new("agent");
       ctx.set_event_sender(pipeline.sender());

       ctx.emit_step_event("my_step", serde_json::json!({"delta": "hi"}));

       let ev = rx.recv().await.unwrap();
       assert!(
           matches!(ev, StepEvent::Chunk { ref step_name, .. } if step_name == "my_step"),
           "expected Chunk, got: {ev:?}"
       );
   }
   ```

   Run: `cargo nextest run -p crux-runtime -- emit_step_event_sends`
   Expected: FAIL (method doesn't exist)

2. Implement on `CruxCtx`:

   ```rust
   /// Emit an intermediate event for a named step.
   ///
   /// The event is both broadcast via the EventPipeline (if attached) and
   /// will be available in the step's `events` vec after recording.
   pub fn emit_step_event(&self, step_name: &str, payload: serde_json::Value) {
       self.emit(StepEvent::Chunk {
           step_name: step_name.to_string(),
           payload,
       });
   }
   ```

3. Verify:

   ```text
   cargo nextest run -p crux-runtime    -> all green
   cargo clippy -p crux-runtime -- -D warnings  -> zero warnings
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-runtime): add emit_step_event for streaming chunks (#76)"`

### Task 7: CitedFinding type (#79)

**Crate**: `crux-types`
**File(s)**: `crates/crux-types/src/step.rs`
**Run**: `cargo nextest run -p crux-types`

1. Write failing test:

   ```rust
   #[test]
   fn step_with_findings_roundtrips() {
       let mut step = step_ok("analyze", 0, None);
       step.findings.push(CitedFinding {
           message: "unused import".into(),
           source: Some("src/lib.rs::main:42".into()),
       });
       let json = serde_json::to_string(&step).unwrap();
       assert!(json.contains("unused import"));
       let back: Step = serde_json::from_str(&json).unwrap();
       assert_eq!(back.findings.len(), 1);
       assert_eq!(back.findings[0].source.as_deref(), Some("src/lib.rs::main:42"));
   }

   #[test]
   fn step_without_findings_omits_field() {
       let step = step_ok("plain", 0, None);
       let json = serde_json::to_string(&step).unwrap();
       assert!(!json.contains("findings"));
   }
   ```

   Run: `cargo nextest run -p crux-types -- findings`
   Expected: FAIL (no findings field)

2. Add `CitedFinding` and the field to `Step`:

   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct CitedFinding {
       pub message: String,
       #[serde(default, skip_serializing_if = "Option::is_none")]
       pub source: Option<String>,
   }
   ```

   Add to `Step`:

   ```rust
   /// Structured diagnostic findings attached during execution.
   #[serde(default, skip_serializing_if = "Vec::is_empty")]
   pub findings: Vec<CitedFinding>,
   ```

3. Update `testing::step_ok` helper to initialize `findings: vec![]`.

4. Verify:

   ```text
   cargo nextest run -p crux-types    -> all green
   cargo clippy -p crux-types -- -D warnings  -> zero warnings
   ```

5. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-types): add CitedFinding type on Step (#79)"`

### Task 8: Crux::to_trace_json() (#83a)

**Crate**: `crux-types`
**File(s)**: `crates/crux-types/src/crux_value.rs`
**Run**: `cargo nextest run -p crux-types`

1. Write failing test:

   ```rust
   #[test]
   fn to_trace_json_produces_presentation_format() {
       let crux = sample_crux();
       let trace = crux.to_trace_json();
       assert_eq!(trace["agent"], "test");
       assert_eq!(trace["status"], "ok");
       assert!(trace["steps"].is_array());
       let steps = trace["steps"].as_array().unwrap();
       assert_eq!(steps[0]["name"], "greet");
       assert_eq!(steps[0]["status"], "ok");
       assert!(steps[0].get("input_hash").is_none(), "hashes omitted");
   }
   ```

   Run: `cargo nextest run -p crux-types -- to_trace_json`
   Expected: FAIL (method doesn't exist)

2. Implement on `Crux<T: Serialize>`:

   ```rust
   /// Produce a presentation-format JSON trace.
   ///
   /// Differs from raw serde: flattens Result, omits hashes, includes
   /// computed fields (total duration). Recursive for children.
   pub fn to_trace_json(&self) -> serde_json::Value {
       let status = match &self.value {
           Ok(_) => "ok",
           Err(_) => "error",
       };
       let steps: Vec<serde_json::Value> = self
           .steps
           .iter()
           .map(|s| {
               let mut obj = serde_json::json!({
                   "name": s.name,
                   "kind": s.kind,
                   "status": s.status,
                   "duration_ms": s.duration_ms,
                   "confidence": s.confidence,
               });
               if let Some(ref err) = s.error {
                   obj["error"] = serde_json::Value::String(err.clone());
               }
               if !s.findings.is_empty() {
                   obj["findings"] = serde_json::to_value(&s.findings)
                       .unwrap_or_default();
               }
               obj
           })
           .collect();
       let children: Vec<serde_json::Value> = self
           .children
           .iter()
           .map(|c| c.to_trace_json())
           .collect();
       let mut trace = serde_json::json!({
           "agent": self.agent,
           "id": self.id.to_string(),
           "status": status,
           "steps": steps,
       });
       if let Some(ms) = self.duration_ms() {
           trace["duration_ms"] = serde_json::json!(ms);
       }
       if !children.is_empty() {
           trace["children"] = serde_json::json!(children);
       }
       trace
   }
   ```

3. Verify:

   ```text
   cargo nextest run -p crux-types    -> all green
   cargo clippy -p crux-types -- -D warnings  -> zero warnings
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-types): add Crux::to_trace_json() (#83)"`

### Task 9: Crux::to_mermaid() (#83b)

**Crate**: `crux-types`
**File(s)**: `crates/crux-types/src/crux_value.rs`
**Run**: `cargo nextest run -p crux-types`

1. Write failing test:

   ```rust
   #[test]
   fn to_mermaid_produces_valid_flowchart() {
       let crux = sample_crux();
       let mermaid = crux.to_mermaid();
       assert!(mermaid.starts_with("graph TD"));
       assert!(mermaid.contains("greet"));
       assert!(mermaid.contains("fill:#90EE90"), "ok steps should be green");
       assert!(
           mermaid.contains("fill:#D3D3D3"),
           "rejected steps should be gray"
       );
   }
   ```

   Run: `cargo nextest run -p crux-types -- to_mermaid`
   Expected: FAIL (method doesn't exist)

2. Implement on `Crux<T: Serialize>`:

   ```rust
   /// Render the execution trace as a Mermaid flowchart.
   ///
   /// Color coding: ok=green, err=red, rejected=gray, skipped=dashed.
   /// Delegation edges annotate the child agent name.
   pub fn to_mermaid(&self) -> String {
       let mut lines = vec!["graph TD".to_string()];
       let mut child_iter = self.children.iter();

       for (i, step) in self.steps.iter().enumerate() {
           let id = format!("s{i}");
           let label = format!("{} {}ms", step.name, step.duration_ms);
           lines.push(format!("    {id}[\"{label}\"]"));

           // Edge from previous step
           if i > 0 {
               let prev = format!("s{}", i - 1);
               if step.kind == StepKind::Delegation {
                   if let Some(child) = child_iter.next() {
                       lines.push(format!(
                           "    {prev} -->|\"delegate: {}\"| {id}",
                           child.agent
                       ));
                   } else {
                       lines.push(format!("    {prev} --> {id}"));
                   }
               } else {
                   let edge_label = match step.status {
                       StepStatus::Ok => "ok",
                       StepStatus::Err => "err",
                       StepStatus::Rejected => "rejected",
                       StepStatus::Skipped => "skipped",
                   };
                   lines.push(format!("    {prev} -->|\"{edge_label}\"| {id}"));
               }
           }

           // Style
           let style = match step.status {
               StepStatus::Ok => "fill:#90EE90",
               StepStatus::Err => "fill:#FF6B6B",
               StepStatus::Rejected => "fill:#D3D3D3",
               StepStatus::Skipped => "fill:#FFFFFF,stroke-dasharray: 5 5",
           };
           lines.push(format!("    style {id} {style}"));
       }

       lines.join("\n")
   }
   ```

3. Verify:

   ```text
   cargo nextest run -p crux-types    -> all green
   cargo clippy -p crux-types -- -D warnings  -> zero warnings
   ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "feat(crux-types): add Crux::to_mermaid() (#83)"`

## Out of Scope

- Cohort B (#71, #73, #74, #77): safety gates, redaction, LLM fallback,
  error ergonomics
- Step output type safety (#81)
- Schema/runtime split (#75)
- EDDOS event aggregation (#78)
- Token-shape priority (#80)
- Unreachable step detection (deferred -- needs data flow analysis beyond
  simple noop check)
