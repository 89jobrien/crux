# crux-agentic

Built-in stateful and service-backed handlers for `crux-script`. This layer adds LLM providers,
containers, CI/review analysis, SQLite, task management, tool discovery, and repository triage on
top of the deterministic `crux-stdlib` handlers.

## Architecture role

`register_all` composes the standard library with this crate's handler modules. Runtime ports are
implemented in `adapters`, including approval gates, LLM provider adapters, and container clients.
Applications may register modules individually or add typed delegated agents with `register_agent`.

```rust
use crux_agentic::register_all;
use crux_script::HandlerRegistry;

let mut registry = HandlerRegistry::new();
register_all(&mut registry);
```

## Handler families

- `llm`: invoke, stream, and provider fallback; structured extraction/planning is feature-gated.
- `container` and `harness`: bounded execution and harness evolution/canary operations.
- `analysis`, `ci`, and `review`: normalize diagnostics, score findings, and propose remediations.
- `triage`: TODO, environment, worktree, sync, and classification transforms.
- `sqlite` and `task`: database CRUD and dependency-aware project task handlers.
- `rx`, `discover`, and plugin integration: external tool/pipeline discovery and invocation.

Public constants in `handlers` provide canonical names such as `LLM_INVOKE`, `CI_NEXTEST_FAILURES`,
`TRIAGE_DETECT_ORPHANED_WORKTREES`, `SQLITE_QUERY_MANY`, and `TASK_READY`.

## Key API

- `register_all`, `register_all_with_plugins`, `register_agent`.
- `LlmProvider`, `LlmRequest`, `LlmResponse`, and `LlmStep`.
- `AutoApproveGate`, `TerminalApprovalGate`, OpenAI/Anthropic/Ollama adapters, and container
  clients.
- Module-level `register` functions for selective installation.

## Features

| Feature | Default | Effect |
| --- | --- | --- |
| `baml` | no | Registers `crux-baml` structured extraction, decomposition, and planning. |
| `docker` | no | Uses Bollard and the local Docker daemon for container handlers. |

Without `docker`, container handlers use a CI-safe mock client rather than running containers.
Provider-backed handlers require their corresponding environment credentials or local service.

## Implementation and generated-code boundary

BAML code is owned and generated in `crux-baml`; this crate contains no `baml_client` directory and
does not generate it. Handler coverage is broad but side effects and credentials vary by family, so
registry metadata should be consulted before policy approval.

## Development and testing

```console
cargo nextest run -p crux-agentic
cargo nextest run -p crux-agentic --features docker
cargo clippy -p crux-agentic --all-targets --all-features -- -D warnings
```

LLM/BAML integration tests need configured providers; mock tests, strict-registration tests, SQLite,
triage, review, CI, and container-default tests run without live API calls.
