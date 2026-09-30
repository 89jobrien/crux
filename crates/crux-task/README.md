# crux-task

Dependency-aware project task management for agents and humans. The library works over the
`crux-runtime` registry port; the companion `crux-task` binary provides persistent CLI workflows.

## Architecture role

`TaskManager<B>` stores `ProjectTask` values through any `RegistryBackend`. The default CLI build
uses redb, the optional SQLite adapter implements the same backend contract, and tests use the
in-memory backend.

## Library usage

```rust
use crux_runtime::registry::InMemoryBackend;
use crux_task::{ProjectTaskStatus, TaskManager, TaskSpec};
use crux_types::task::Priority;

let manager = TaskManager::new(InMemoryBackend::new());
let id = manager.add(TaskSpec {
    title: "Write tests".into(),
    description: None,
    priority: Priority::P1,
    status: ProjectTaskStatus::Open,
    labels: vec![],
    dependencies: vec![],
}).await?;
# Ok::<(), crux_task::TaskErr>(())
```

`TaskManager` supports add/get/update/list, ready/blocked queries, priority ordering, dependency
block/unblock, and aggregate statistics. Public data types include `TaskSpec`, `TaskPatch`,
`TaskFilter`, `ProjectTask`, `Dependency`, status, and stats.

## CLI

```console
crux-task add "Write tests" --priority p1 --labels docs,release
crux-task list --ready
crux-task show <task-id>
crux-task update <task-id> --status in-progress --add-label active
crux-task block <task-id> --by <blocker-id>
crux-task unblock <task-id> --from <blocker-id>
crux-task stats --json
```

Global options are `--json`, `--db <PATH>`, and `--sqlite <PATH>`. The redb path is selected by
`--db`, then `CRUX_TASK_DB`, then the platform data directory under `crux-task/tasks.redb`.

## Features

| Feature | Default | Effect |
| --- | --- | --- |
| `redb` | yes | Enables persistent redb storage and the default CLI backend. |
| `sqlite` | no | Enables `SqliteBackend`; the parsed `--sqlite` flag otherwise has no adapter. |

## Implementation status

Task IDs and shared priority/dependency values come from `crux-types`. Ready means open and not
blocked by an incomplete dependency. There is no generated source. CLI `--json` applies fully to
add/list/show/ready/stats; mutation commands currently print human-readable confirmations.

## Development and testing

```console
cargo nextest run -p crux-task
cargo nextest run -p crux-task --all-features
cargo clippy -p crux-task --all-targets --all-features -- -D warnings
```

Workflow and backend-conformance tests cover dependency chains, filtering, persistence behavior,
priority ordering, and statistics.
