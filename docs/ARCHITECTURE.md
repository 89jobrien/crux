# Handler Architecture

This document explains the handler registry, contract-based loading pattern, and the relationship between step handlers and plugin manifests in crux pipelines.

## Overview

Crux pipelines execute steps by looking up handlers from a registry. Every handler is a typed async closure that receives a JSON input and returns either a result or an error. The runtime never calls handlers directly — it always uses the registry to dispatch them by name.

```
Pipeline YAML → HandlerName (string) → HandlerRegistry → Handler (async closure) → HandlerOutput
```

## Handler Registry

The `HandlerRegistry` (in `crux-script`) is the central dispatch table:

```rust
pub struct HandlerRegistry {
    handlers: HashMap<String, Arc<dyn HandlerFn>>,
    metadata: HashMap<String, HandlerMetadata>,
}

impl HandlerRegistry {
    pub fn handler<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<HandlerOutput, CruxErr>> + Send + 'static,
    { ... }

    pub fn handler_value<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Value> + Send + 'static,
    { ... }

    pub fn invoke(&self, name: &str, input: Value) -> HandlerFuture { ... }
}
```

Two registration methods exist:

### 1. `registry.handler(name, closure)`

Closure returns `Result<HandlerOutput, CruxErr>`. Preferred for handlers that:
- Score their own outputs (emit confidence)
- Can fail with meaningful error messages
- Need to report usage (tokens, cost)

```rust
registry.handler(
    HandlerMetadata::new("analysis::tighten_budget")
        .describe("Suggest tighter budget")
        .risk(RiskLevel::Low),
    |input: Value| async move {
        let budget = calculate_tighter_budget(&input)?;
        Ok(HandlerOutput::with_confidence(budget, 0.92))
    },
);
```

### 2. `registry.handler_value(name, closure)`

Closure returns plain `Value`. Auto-wrapped with `confidence = None` (defaults to `0.5`). Used for:
- Simple transforms (no error paths)
- Deterministic operations
- Legacy handlers

```rust
registry.handler_value(
    "json::pick",
    |input: Value| async move {
        input.get("selected_fields").cloned().unwrap_or(Value::Null)
    },
);
```

## Contract-Based Loading Pattern

Handlers are registered in modules by domain. Each module implements a `register()` function:

```rust
pub fn register(registry: &mut HandlerRegistry) {
    register_latency_profile(registry);
    register_token_spend(registry);
    register_failure_clusters(registry);
    // ... more handlers
}
```

The **main entry point** in `crux-agentic` is:

```rust
pub fn register_all(registry: &mut HandlerRegistry) {
    // Stdlib (shell, fs, git, json, text, ctrl)
    crux_stdlib::register_all(registry);

    // Agentic domains
    analysis::register(registry);
    ci::register(registry);
    container::register(registry);
    harness::register(registry);
    http::register(registry);
    review::register(registry);
    rx::register(registry);
    sqlite::register(registry);
    task::register(registry);
    triage::register(registry);

    // LLM handlers (routed through BAML for provider selection)
    crux_baml::register_all_with_plugins(registry, plugin_handlers);

    // TypeSafe judgment handlers (conditional on API key)
    register_judgment_handlers(registry);
}
```

This design ensures:

1. **Compile-time verification** — all handlers are Rust functions; typos caught at build time
2. **Domain isolation** — each module registers only handlers in its namespace
3. **Feature gates** — optional handlers (like `judge::score`) are gated on environment
4. **Plugin integration** — plugin manifests are passed to BAML for planner awareness

## Handler Invocation Flow

```
Pipeline execution
    ↓
Step definition (YAML): "handler: llm::analyze"
    ↓
HandlerRegistry::invoke("llm::analyze", input)
    ↓
Lookup handler closure in map
    ↓
Call: handler(input) → Future
    ↓
Await result: Result<HandlerOutput, CruxErr>
    ↓
Record step: Step {name, status, output, confidence, ...}
    ↓
Next step or branch (route_on_confidence)
```

## Relationship: Step Handlers vs Plugin Manifests

### Step Handlers

A "step handler" is the runtime entity — a registered closure in the `HandlerRegistry`:

```yaml
- step: extract_issues
  handler: llm::extract     # ← This name must exist in registry
  args:
    subject: "CI output"
    extraction_type: "ClassifyCIFailure"
```

When executed, `HandlerRegistry::invoke("llm::extract", args)` is called.

### Plugin Manifests

A "plugin manifest" is metadata about available handlers for the **planner**:

```toml
# ~/.crux/plugins/my_handler.toml

[handler]
name = "my_domain::my_handler"
description = "Does something useful"
risk_level = "medium"

[handler.input]
type = "object"
properties.subject = { type = "string" }
properties.evidence = { type = "string" }

[handler.output]
type = "object"
properties.result = { type = "string" }
properties.confidence = { type = "number" }
```

When planning (`crux plan --goal "..."`), the planner:
1. Loads all plugin manifests
2. Reads the registry to discover built-in handlers
3. Uses both to decide which handler to use in the generated pipeline

**Built-in handlers** (like `llm::extract`) are registered in code and discovered by the planner at runtime. The planner has built-in knowledge of common domains (llm, shell, fs, git, json).

**Plugin handlers** (like `custom::analyze`) are described by manifest TOML files. The planner uses the manifest schema to understand input/output types.

## Architecture of crux-agentic

```
crux-agentic/
├── src/
│   ├── lib.rs              # Entry: register_all()
│   ├── handlers.rs         # Compile-time constants (handler names)
│   ├── analysis.rs         # Trace analysis handlers
│   ├── ci.rs               # CI log parsing handlers
│   ├── container.rs        # Container lifecycle handlers
│   ├── harness.rs          # Harness orchestration handlers
│   ├── http.rs             # HTTP utilities
│   ├── review.rs           # PR review handlers
│   ├── rx.rs               # Reactive step handlers
│   ├── sqlite.rs           # SQLite persistence
│   ├── task.rs             # Task management
│   ├── triage.rs           # Issue triage handlers
│   ├── llm.rs              # LLM provider interface
│   ├── llm_step.rs         # LLM step types
│   └── adapters/           # ApprovalGate, SafetyPolicy implementations
└── baml_src/               # BAML specs for structured LLM output
```

### Domain Modules

Each domain module follows the same pattern:

```rust
// crates/crux-agentic/src/analysis.rs

const SLOW_STEP_MULTIPLIER: f64 = 2.0;  // Configuration constants

pub fn register(registry: &mut HandlerRegistry) {
    register_latency_profile(registry);
    register_token_spend(registry);
    // ...
}

fn register_latency_profile(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_LATENCY_PROFILE)
            .describe("Profile step latencies and identify outliers")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            // Implementation
        },
    );
}
```

**Key patterns:**
- Metadata includes description, risk level, determinism flag
- Use compile-time constants from `handlers.rs` for handler names
- Async closures accept `Value` and return `Result<HandlerOutput, CruxErr>`
- Error messages should be actionable

## Handler Metadata

Every handler can declare metadata for the planner and CLI:

```rust
HandlerMetadata::new("domain::name")
    .describe("Human-readable description")
    .risk(RiskLevel::Low | Medium | High)
    .deterministic(true)  // Can be replayed without side effects
    .inputs(/* JSON Schema */)
    .outputs(/* JSON Schema */)
```

The planner uses this to:
- Decide whether to use the handler in generated pipelines
- Construct correct input/output transforms
- Warn if a handler is risky or non-deterministic

## LLM Handlers and BAML

All `llm::*` handlers are owned by `crux-baml`, which:

1. Selects a provider (OpenAI, Anthropic, Ollama, fallback chain)
2. Retries on transient failures
3. Parses and validates structured output
4. Reports confidence with retry penalty: `self_report * 0.8^(attempts-1)`

The `llm_step.rs` module in `crux-agentic` provides Rust types for common LLM workflows, but the actual handler implementations live in BAML.

Configuration:
- `crates/crux-baml/baml_src/clients.baml` — provider chains
- Environment: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OLLAMA_BASE_URL`

## Conditional Registration

Some handlers are registered only when certain conditions are met:

### TypeSafe Judgment Handlers

`judge::score` is registered only when `TYPESAFE_API_KEY` environment variable is set.

```rust
fn register_judgment_handlers(registry: &mut HandlerRegistry) {
    let Some(client) = typesafe_judgment_client() else {
        return;  // Key not set; skip registration
    };
    crux_typesafe::register(registry, client);
}
```

This ensures:
- Unconfigured workspaces see exactly the handler set they had before
- `crux handlers list` does not show unavailable handlers
- Pipelines naming the handler get a clear `unknown_handler` error message

## Step Handler vs Delegate Handler

Two related but distinct concepts:

### Step Handler

A handler registered in the `HandlerRegistry`. Invoked by name from YAML:

```yaml
- step: analyze
  handler: llm::analyze     # ← Step handler
  args: { ... }
```

### Delegate Handler

An agent registered for `delegate:` pipeline steps. Also a closure, but:
- Takes a `Value` input (whole step args)
- Returns `Result<Value, CruxErr>`
- Produces a child `Crux<Value>` trace

```yaml
- delegate:
    agent: my_agent         # ← Delegate handler (registered separately)
    input: { ... }
```

Delegates are registered via:

```rust
pub fn register_agent<F, Fut>(registry: &mut HandlerRegistry, name: String, f: F)
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
{ ... }
```

## Testing Handlers

Handlers are unit-tested by:
1. Creating a `HandlerRegistry`
2. Registering the handler
3. Invoking it with test inputs
4. Asserting output

```rust
#[tokio::test]
async fn test_tighten_budget() {
    let mut registry = HandlerRegistry::new();
    analysis::register(&mut registry);

    let input = json!({
        "steps": [...],
        "budget": {...}
    });

    let result = registry.invoke("analysis::tighten_budget", input).await;
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(output.confidence > 0.8);
}
```

## Errors and Recovery

Handlers return errors as `Err(CruxErr)`. The pipeline runtime then:

1. **Checks** if the step has `allow_failure: true`
2. **If yes:** wraps the error in a value and continues
3. **If no:** propagates the error and fails the step
4. **Retries:** if the step declares `retry: {count, delay_ms}`, reattempts up to `count` times

```yaml
- step: risky_operation
  handler: some::handler
  args: { ... }
  allow_failure: true    # ← Error becomes a step value
  retry:                 # ← Retried 3 times before giving up
    count: 3
    delay_ms: 1000
```

The error message is preserved in the `Step` record for debugging.

## Performance Considerations

### Async Runtime

All handlers run in a shared `tokio` runtime. Handlers should:
- Avoid blocking operations (use `tokio::fs`, `tokio::process`, etc.)
- Use timeouts for network calls (`tokio::time::timeout`)
- Batch I/O when possible

### Caching and Memoization

The `CruxCtx` maintains a `ReplayCache` that caches step outputs by name + ordinal hash. In `lenient` mode (the default), cache misses scan forward for the next matching step name.

Handlers don't need to implement caching — the runtime handles it.

### Budget Tracking

Handlers can report usage via `HandlerUsage`:

```rust
HandlerExecution::success(output, HandlerUsage::metered(tokens, usd_amount))
```

The runtime aggregates usage per step and enforces the pipeline's budget.

## Extension Points

Handlers can be extended without modifying core crux:

1. **Custom handlers** — register domain-specific closures in the registry
2. **Plugin system** — subprocess plugins expose handlers over stdio
3. **BAML extensions** — add new LLM structured-output functions in `generators.baml`
4. **Adapters** — implement `SafetyPolicy`, `ApprovalGate`, `TrustRegistry` traits

See [crux-plugins.md](crux-plugins.md) for plugin architecture details.
