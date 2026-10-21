# Handler Conformance Specification

This document defines the contract, input/output types, and error handling requirements for all built-in and custom crux handlers.

## Handler Contract

Every handler must conform to the following interface:

```rust
pub type HandlerFn = fn(input: Value) -> HandlerOutput;
pub type HandlerAsyncFn = fn(input: Value) -> impl Future<Output = HandlerOutput>;
```

### HandlerOutput

```rust
pub struct HandlerOutput {
    pub value: Value,
    pub confidence: Option<f32>,  // 0.0..1.0; None defaults to 0.5
}
```

**Key requirements:**

- **Value**: A `serde_json::Value` (JSON-compatible) containing the handler's output
- **Confidence**: Optional `f32` in the range `[0.0, 1.0]`
  - Handlers that score their own outputs should emit a value
  - Handlers without confidence emit `None` (defaults to neutral `0.5`)
  - Used by `route_on_confidence` to make branching decisions
  - LLM handlers report `confidence = self_report * 0.8^(attempts-1)` (includes retry penalty)

### Error Handling

Handlers return `Result<HandlerOutput, CruxErr>` via async:

```rust
#[async_trait]
impl Handler {
    async fn invoke(&self, input: Value) -> Result<HandlerOutput, CruxErr> { ... }
}
```

**CruxErr variants for handler failures:**

| Variant | Usage |
|---------|-------|
| `step_failed(handler_name, message)` | Handler logic failed; message explains why |
| `budget_exceeded(budget_name, spent, limit)` | Exceeded token, call, or cost budget |
| `timeout` | Handler exceeded `timeout_ms` |
| `unknown_handler` | Handler name not found in registry |
| `parse_error` | Input JSON did not match expected schema |
| `schema_validation_failed` | Output did not match declared JSON Schema |

Handlers should:
1. Return `Err` for failures that cannot be recovered
2. Return `Ok` with an error-describing value for `allow_failure: true` steps
3. Include actionable messages in error text

## Built-in Handler Categories

### Shell Handlers

**Domain:** `shell`

These execute shell commands and capture output.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `shell::exec` | `{cmd: string, [cwd]: string, [env]: {string: string}}` | `{stdout: string, stderr: string, exit_code: number}` | None | step_failed if cmd syntax invalid |
| `shell::capture` | same | same | None | step_failed if exit_code != 0 |

**Examples:**
```yaml
- step: list_files
  handler: shell::exec
  args:
    cmd: "ls -la"
    cwd: "/tmp"

- step: format_check
  handler: shell::capture
  args:
    cmd: "cargo fmt --all -- --check"
```

### Filesystem Handlers

**Domain:** `fs`

Read, write, and query the filesystem.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `fs::read` | `{path: string}` | `string` (file contents) | None | step_failed if file not found or not readable |
| `fs::write` | `{path: string, content: string}` | `{path: string, bytes_written: number}` | None | step_failed if write failed |
| `fs::glob` | `{pattern: string, [base]: string}` | `[string]` (matching paths) | None | step_failed if pattern invalid |
| `fs::exists` | `{path: string}` | `boolean` | None | Never fails |

**Examples:**
```yaml
- step: read_config
  handler: fs::read
  args:
    path: "config.toml"

- step: find_tests
  handler: fs::glob
  args:
    pattern: "tests/**/*.rs"
    base: "."
```

### Git Handlers

**Domain:** `git`

Query git state (does not modify the repository).

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `git::staged_files` | `{}` | `[string]` (file paths) | None | step_failed if .git not found |
| `git::diff` | `{[revision]: string}` | `string` (unified diff) | None | step_failed if revision invalid |
| `git::log` | `{[count]: number, [format]: string}` | `[{hash: string, message: string, author: string}]` | None | step_failed if count negative |
| `git::status` | `{}` | `[{path: string, status: string}]` | None | step_failed if .git not found |

**Examples:**
```yaml
- step: get_changes
  handler: git::diff
  args:
    revision: "main"

- step: recent_commits
  handler: git::log
  args:
    count: 10
```

### JSON Handlers

**Domain:** `json`

Transform and query JSON structures.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `json::pick` | `{fields: [string]}` on object input | object (selected fields) | None | step_failed if non-object input |
| `json::merge` | input object + `{with: object}` | merged object | None | step_failed if non-object input |
| `json::jq` | input value + `{query: string}` | transformed value | None | step_failed if query syntax invalid |
| `json::group_by` | input array + `{key: string}` | object (groups) | None | step_failed if non-array input |
| `json::filter_nonempty` | input array + `{field: string}` | filtered array | None | step_failed if non-array input |

**jq query support:**
- Dot-path: `.foo.bar`
- Array indexing: `.[0]`, `.[0][1]`
- Pipes: `| length`, `| keys`
- Selection: `select(.status == "active")`
- Mapping: `map(.name)`

**Examples:**
```yaml
- step: extract_names
  handler: json::pick
  args:
    fields: [name, email, id]

- step: query_data
  handler: json::jq
  args:
    query: ".items[] | select(.active == true) | .id"
```

### Control Flow Handlers

**Domain:** `ctrl`

Lightweight control flow and debugging.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `ctrl::noop` | any | input unchanged | None | Never fails |
| `ctrl::log` | any | input unchanged | None | Never fails |
| `ctrl::assert` | any + `{condition: string}` | input if true | None | step_failed if condition false |

**Examples:**
```yaml
- step: debug
  handler: ctrl::log
  args:
    message: "Processing user {{ input.id }}"

- step: validate
  handler: ctrl::assert
  args:
    condition: "{{ input.count > 0 }}"
```

### LLM Handlers

**Domain:** `llm`

All LLM handlers are routed through BAML for unified provider selection, retries, and parsing. All report confidence scores.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `llm::invoke` | `{prompt: string, [schema]: object, [client]: string, [model]: string}` | `{content: string, ...schema_fields, confidence: number, attempts: number}` | **Yes (reported)** | step_failed if provider unavailable or parsing failed |
| `llm::stream` | same | `{content: string, chunks: [string], confidence: number, attempts: number}` | **Yes (reported)** | same |
| `llm::extract` | `{subject: string, extraction_type: string, evidence: string}` | `{entities: [...],...}` (typed) | **Yes (reported)** | step_failed if extraction failed |
| `llm::analyze` | `{subject: string, evidence: string, [focus]: string}` | `{summary: string, findings: [...], severity: string}` | **Yes (reported)** | step_failed if analysis failed |
| `llm::confidence` | `{claim: string, evidence: string, [criteria]: string}` | `{score: 0.0..1.0, reasoning: string}` | **Yes (reported)** | step_failed if scoring failed |
| `llm::decompose` | `{spec: string}` | `{tasks: [{title, description, acceptance_criteria}]}` | **Yes (reported)** | step_failed if decomposition failed |
| `llm::plan` | `{goal: string}` | `{steps: [{handler, args, expected_output}]}` | **Yes (reported)** | step_failed if planning failed |

**Confidence calculation:**
```
confidence = self_report * 0.8^(attempts - 1)
```

**Examples:**
```yaml
- step: extract_entities
  handler: llm::extract
  args:
    subject: "{{ input.text }}"
    extraction_type: "ExtractEntities"
    evidence: "The entities mentioned in the document"

- step: score_claim
  handler: llm::confidence
  args:
    claim: "{{ input.hypothesis }}"
    evidence: "{{ steps.analysis.output }}"
    criteria: "Evidence directly supports the claim"
```

### Container Handlers

**Domain:** `container`

Lifecycle management for containers and harnesses.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `container::run` | `{image: string, [env]: object, [limits]: object, [timeout_ms]: number}` | `{container_id: string, status: string}` | None | step_failed if image not found or start failed |
| `container::wait` | `{container_id: string, [timeout_ms]: number}` | `{exit_code: number, stdout: string, stderr: string}` | None | step_failed if container not found or wait timeout |
| `harness::evolve` | `{metrics: RunMetrics}` | `{diff: HarnessDiff, confidence: number}` | **Yes** | step_failed if evolution planning failed |
| `harness::canary` | `{image: string, traffic_percent: number}` | `{canary_id: string, traffic: number}` | None | step_failed if canary deploy failed |

**Examples:**
```yaml
- step: start_service
  handler: container::run
  args:
    image: "my-service:latest"
    env:
      DEBUG: "true"
    limits:
      memory: "512Mi"
      cpu: "250m"
```

### Analysis Handlers

**Domain:** `analysis`

Analyze completed traces to identify performance and reliability issues.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `analysis::latency_profile` | `{steps: [...]}` | `{slow_steps: [...], median_ms: number}` | None | Never fails |
| `analysis::token_spend` | `{steps: [...]}` | `{total_tokens: number, top_consumers: [...]}` | None | Never fails |
| `analysis::failure_clusters` | `{steps: [...]}` | `{clusters: [{error_kind: string, count: number}]}` | None | Never fails |
| `analysis::replay_cache_hits` | `{steps: [...]}` | `{hit_rate: number, per_step: object}` | None | Never fails |
| `analysis::tighten_budget` | `{steps: [...], budget: Budget}` | `{suggested_budget: Budget, confidence: number}` | **Yes** | Never fails |
| `analysis::compress_stages` | `{steps: [...]}` | `{heavy_stages: [string]}` | None | Never fails |
| `analysis::tune_retry` | `{steps: [...]}` | `{retry_suggestions: [{step: string, count: number}]}` | None | Never fails |

**Examples:**
```yaml
- step: profile_latency
  handler: analysis::latency_profile
  args:
    steps: "{{ crux.steps }}"

- step: suggest_tighter_budget
  handler: analysis::tighten_budget
  args:
    steps: "{{ crux.steps }}"
    budget: "{{ input.budget }}"
```

### CI Handlers

**Domain:** `ci`

Parse CI logs and triage failures.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `ci::compile_errors` | `{log: string}` | `{errors: [{code: string, message: string, file: string, line: number}]}` | None | Never fails |
| `ci::clippy_violations` | `{log: string}` | `{violations: [{lint: string, message: string, file: string}]}` | None | Never fails |
| `ci::nextest_failures` | `{log: string}` | `{failures: [{test: string, panic_msg: string}]}` | None | Never fails |
| `ci::deny_violations` | `{log: string}` | `{violations: [{kind: string, id: string, reason: string}]}` | None | Never fails |
| `ci::classify_severity` | `{findings: [...]}` | `{findings: [...], max_severity: string}` | None | Never fails |
| `ci::attach_owners` | `{findings: [...]}` | `{findings: [{...crate: string}]}` | None | step_failed if cargo metadata unavailable |
| `ci::score_fixability` | `{findings: [...]}` | `{findings: [{...fixability_score: number}], confidence: number}` | **Yes** | Never fails |

**Examples:**
```yaml
- step: parse_errors
  handler: ci::compile_errors
  args:
    log: "{{ steps.build.output.stderr }}"

- step: attach_owners
  handler: ci::attach_owners
  args:
    findings: "{{ steps.parse.output.errors }}"
```

### Review Handlers

**Domain:** `review`

PR review signal normalization and scoring.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `review::arch_boundary_check` | `{repo_path: string, diff: string}` | `{violations: [{file: string, import: string, reason: string}]}` | None | step_failed if rg not found |
| `review::normalize_findings` | `{clippy: [...], coverage: [...], arch: [...]}` | `[Finding]` | None | Never fails |
| `review::apply_severity` | `{findings: [...]}` | `{findings: [{...severity: string}]}` | None | Never fails |
| `review::compute_score` | `{findings: [...]}` | `{score: number}` | None | Never fails |
| `review::approve` | `{pr_number: number, [comment]: string}` | `{status: string, review_id: string}` | None | step_failed if gh CLI unavailable |

**Examples:**
```yaml
- step: check_architecture
  handler: review::arch_boundary_check
  args:
    repo_path: "."
    diff: "{{ steps.git_diff.output }}"

- step: normalize
  handler: review::normalize_findings
  args:
    clippy: "{{ steps.lint.output.findings }}"
    coverage: "{{ steps.coverage.output.gaps }}"
    arch: "{{ steps.arch_check.output.violations }}"
```

### Triage Handlers

**Domain:** `triage`

Doob backlog processing and prioritization.

| Handler | Input | Output | Confidence | Errors |
|---------|-------|--------|-----------|--------|
| `triage::parse_repo_tags` | `{todos: [...]}` | `{todos: [{...repo: string}]}` | None | Never fails |
| `triage::score_urgency` | `{todos: [...]}` | `{todos: [...], sorted: [string]}` | None | Never fails |
| `triage::deduplicate_intent` | `{todos: [...]}` | `{clusters: [{items: [...], representative: Todo}]}` | None | Never fails |
| `triage::group_by_repo` | `{todos: [...]}` | `{groups: {[repo]: [Todo]}}` | None | Never fails |

**Examples:**
```yaml
- step: parse_repos
  handler: triage::parse_repo_tags
  args:
    todos: "{{ input.todos }}"

- step: prioritize
  handler: triage::score_urgency
  args:
    todos: "{{ steps.parse.output.todos }}"
```

## Custom Handler Requirements

To register a custom handler:

```rust
pub fn register_my_handler(registry: &mut HandlerRegistry) {
    registry.handler(
        HandlerMetadata::new("my_domain::my_handler")
            .describe("What this handler does")
            .risk(RiskLevel::Medium),
        |input: Value| async move {
            // Validate input, perform work
            Ok(HandlerOutput::new(result_value))
            // or
            Ok(HandlerOutput::with_confidence(result_value, 0.85))
            // or
            Err(CruxErr::step_failed("my_handler", "detailed error message"))
        },
    );
}
```

### Guidelines

1. **Input validation:** Handlers should validate their inputs before processing
2. **Confidence:** Emit when the handler can meaningfully score output quality
3. **Error messages:** Include enough detail for debugging; pipeline authors will log these
4. **Async:** Handlers are async by default; use `tokio::time::timeout` for timeouts
5. **Determinism:** Mark deterministic handlers so they can be replayed safely
6. **Cost tracking:** Report `HandlerUsage` for paid calls (LLM, API calls)

## Testing Handlers

Use the `crux-script` test harness:

```rust
#[tokio::test]
async fn test_my_handler() {
    let mut registry = HandlerRegistry::new();
    register_my_handler(&mut registry);

    let input = json!({"key": "value"});
    let result = registry.invoke("my_domain::my_handler", input).await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output.value, json!({"result": "..."}));
}
```

## Known Limitations

- `handler_value` handlers (plain `Value` output) carry no confidence; they default to `0.5`
- `route_on_confidence` requires handlers to emit scores for meaningful branching
- Some handlers (like `json::jq`) implement a subset of their full spec (no `reduce`, `foreach`, variable bindings, etc.)
- `for_each: parallel` is accepted but currently executes sequentially

## Version Stability

| Handler | Stability | Notes |
|---------|-----------|-------|
| `shell::*`, `fs::*`, `git::*`, `json::*` | Stable | Core handlers; minimal breaking changes |
| `ctrl::*`, `llm::*` | Stable | Fundamental; breaking changes require major version |
| `container::*`, `harness::*` | Beta | Expected to evolve with container orchestration |
| `analysis::*`, `ci::*`, `review::*`, `triage::*` | Experimental | May change signature or be reorganized |
| `judge::*` | Experimental | New; pending TypeSafe integration stabilization |
