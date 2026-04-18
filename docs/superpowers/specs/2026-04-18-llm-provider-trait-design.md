# LlmProvider Trait + LlmStep + minibox-agent Adapter

**Date:** 2026-04-18
**Status:** Approved
**Repos:** crux, minibox

---

## Goal

Replace the hand-rolled HTTP code in `crux-agentic/src/llm.rs` with a proper `LlmProvider`
port and concrete adapters. Add a generic `LlmStep<P>` for use in typed `#[agent]` code.
Implement `LlmProvider` for minibox-llm's `FallbackChain` in `minibox-agent`, making that
crate a thin adapter layer rather than a self-contained HTTP client.

---

## Architecture

```
cruxai-core          — unchanged (Agent, Context, CruxErr, Budget)
crux-agentic 0.2.3   — gains: LlmProvider trait, LlmRequest/LlmResponse types,
                               AnthropicAdapter, OpenAiAdapter, LlmStep<P>
                       changes: llm.rs handler rewired through trait adapters
minibox-agent        — implements LlmProvider for FallbackChain; re-exports LlmStep
minibox-llm          — unchanged
```

Dependencies point inward. `crux-agentic` has no knowledge of minibox. `minibox-agent`
depends on both `crux-agentic` and `minibox-llm`.

---

## crux-agentic Changes

### New file: `src/provider.rs`

Defines the port and its domain types:

```rust
pub trait LlmProvider: Send + Sync + 'static {
    fn complete(
        &self,
        req: LlmRequest,
    ) -> impl std::future::Future<Output = Result<LlmResponse, CruxErr>> + Send;
}

pub struct LlmRequest {
    pub prompt: String,
    pub system: Option<String>,
    pub max_tokens: u32,
}

impl Default for LlmRequest { ... } // max_tokens = 1024

pub struct LlmResponse {
    pub text: String,
    pub provider: String, // e.g. "anthropic/claude-sonnet-4-6"
}
```

`LlmRequest` and `LlmResponse` must derive `Serialize + DeserializeOwned` so they can pass
through `ctx.step`'s replay boundary.

### New file: `src/llm_step.rs`

Generic adapter that drives any `LlmProvider` through the crux `Context`:

```rust
pub struct LlmStep<P: LlmProvider> {
    provider: Arc<P>,
}

impl<P: LlmProvider> LlmStep<P> {
    pub fn new(provider: P) -> Self { ... }

    pub async fn invoke(
        &self,
        ctx: &mut CruxCtx,
        step_name: &str,
        req: LlmRequest,
    ) -> Result<LlmResponse, CruxErr> {
        let p = Arc::clone(&self.provider);
        ctx.step(step_name, move || async move { p.complete(req).await }).await
    }
}
```

### New file: `src/adapters/anthropic.rs`

Extract the existing `complete_anthropic` function from `llm.rs` into a struct:

```rust
pub struct AnthropicAdapter { api_key: String, model: String, base_url: String }

impl AnthropicAdapter {
    pub fn from_env() -> Self { ... } // reads ANTHROPIC_API_KEY
}

impl LlmProvider for AnthropicAdapter { ... }
```

### New file: `src/adapters/openai.rs`

Same pattern for OpenAI:

```rust
pub struct OpenAiAdapter { api_key: String, model: String, base_url: String }

impl OpenAiAdapter {
    pub fn from_env() -> Self { ... } // reads OPENAI_API_KEY
}

impl LlmProvider for OpenAiAdapter { ... }
```

### Modified: `src/llm.rs`

The `llm::complete` and `llm::extract` script handlers are **rewired** to use the adapter
structs. The raw reqwest calls are removed. External behavior (input JSON shape, output JSON
shape) is unchanged — existing crux-script pipelines continue to work without modification.

```rust
// llm::complete handler: instantiate adapter from "provider" field, call .complete()
registry.handler("llm::complete", |input: Value| async move {
    let provider = opt_str(&input, "provider").unwrap_or("openai");
    let req = LlmRequest::from_json(&input)?;
    match provider {
        "anthropic" => AnthropicAdapter::from_env().complete(req).await,
        _ => OpenAiAdapter::from_env().complete(req).await,
    }
    .map(|r| json!({ "content": r.text, "provider": r.provider }))
});
```

### `src/lib.rs`

Export the new public surface:

```rust
pub mod adapters;
pub mod provider;
pub mod llm_step;
pub use provider::{LlmProvider, LlmRequest, LlmResponse};
pub use llm_step::LlmStep;
```

### Version bump

`crux-agentic`: 0.2.2 → 0.2.3
`cruxai-core`, `cruxai`, `cruxai-script`, `cruxai-macros`: remain 0.2.1 (no changes)

---

## minibox-agent Changes

### `Cargo.toml`

Replace the current `cruxai` + `cruxai-core` deps with:

```toml
crux-agentic = "0.2.3"
cruxai-core = "0.2.1"
minibox-llm = { path = "../minibox-llm" }
```

### New file: `src/provider.rs`

Newtype adapter implementing crux's `LlmProvider` for minibox-llm's `FallbackChain`:

```rust
pub struct FallbackChainAdapter(Arc<FallbackChain>);

impl FallbackChainAdapter {
    pub fn from_env() -> Self { Self(Arc::new(FallbackChain::from_env())) }
    pub fn new(chain: Arc<FallbackChain>) -> Self { Self(chain) }
}

impl crux_agentic::LlmProvider for FallbackChainAdapter {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, CruxErr> {
        self.0
            .complete(&CompletionRequest {
                prompt: req.prompt,
                system: req.system,
                max_tokens: req.max_tokens,
                ..Default::default()
            })
            .await
            .map(|r| LlmResponse { text: r.text, provider: r.provider })
            .map_err(|e| CruxErr::step_failed("minibox_llm", e.to_string()))
    }
}
```

### Modified: `src/step.rs`

Delete `CruxLlmStep` struct. Replace with a newtype (type aliases cannot have inherent
methods in stable Rust):

```rust
pub struct CruxLlmStep(crux_agentic::LlmStep<FallbackChainAdapter>);

impl CruxLlmStep {
    pub fn from_env() -> Self {
        Self(crux_agentic::LlmStep::new(FallbackChainAdapter::from_env()))
    }

    pub async fn invoke(
        &self,
        ctx: &mut CruxCtx,
        step_name: &str,
        req: LlmRequest,
    ) -> Result<LlmResponse, CruxErr> {
        self.0.invoke(ctx, step_name, req).await
    }
}
```

### Modified: `src/lib.rs`

```rust
pub mod error;
pub mod provider;
pub mod step;

pub use crux_agentic::{LlmProvider, LlmRequest, LlmResponse, LlmStep};
pub use error::AgentError;
pub use provider::FallbackChainAdapter;
pub use step::CruxLlmStep;

// Re-export crux agent macro for one-dep convenience
pub use cruxai_core::{agent::Agent, ctx::CruxCtx, types::error::CruxErr};
```

---

## Error Mapping

| Source error          | Mapped to                                       |
|-----------------------|-------------------------------------------------|
| `LlmError` (minibox)  | `CruxErr::step_failed("minibox_llm", msg)`      |
| `CruxErr` (crux)      | `AgentError::Step(msg)` (in minibox-agent)      |

---

## Testing

**crux-agentic:**
- Unit test `AnthropicAdapter` and `OpenAiAdapter` with `wiremock` (already a dev-dep in minibox;
  add to crux dev-deps)
- Unit test `LlmStep` with a `MockLlmProvider` that returns a fixed `LlmResponse`
- Verify `llm::complete` handler output shape is unchanged (regression test against existing
  crux-script pipeline tests)

**minibox-agent:**
- Unit test `FallbackChainAdapter::complete` with a mock `FallbackChain` (or a test-doubles
  `LlmProvider` impl)
- Verify `CruxLlmStep::from_env()` constructs without panic in a test env with dummy API keys

---

## Publish Sequence

1. Implement all crux-agentic changes in the crux repo
2. Run `cargo test -p crux-agentic` — all green
3. Bump crux-agentic version to 0.2.3 in `Cargo.toml`
4. `cargo publish -p crux-agentic`
5. Update minibox-agent `Cargo.toml`: `crux-agentic = "0.2.3"`
6. Implement `FallbackChainAdapter` and update `step.rs` / `lib.rs`
7. `cargo check -p minibox-agent` — green
8. Commit both repos

---

## Out of Scope

- Gemini adapter in crux-agentic (minibox-llm handles Gemini; crux stays focused on
  Anthropic + OpenAI for now)
- Streaming LLM responses through `ctx.step_stream` (future work)
- Publishing minibox-agent to crates.io (it is a workspace-internal crate for now)
