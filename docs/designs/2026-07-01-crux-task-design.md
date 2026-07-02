# Design: crux-task — Project Task Management System

## Goal

Build a task management system within crux that provides priority-based,
dependency-aware task tracking usable both by crux agents at runtime and by
developers from the CLI, independent of doob.

## Approved Approach

Domain Split — the runtime `TaskRegistry` stays lean for agent checkpointing.
A new `crux-task` crate builds its own `TaskManager` with richer domain types,
its own persistence (redb + SQLite), a CLI binary, and LLM-powered goal
decomposition. Pipeline step handlers in `crux-agentic` bridge pipelines to the
task system.

## Crate Ownership

| Crate | Role | New? |
|---|---|---|
| `crux-types` | Wire-format types: `Priority`, `TaskLabel`, `DependencyKind` | Extend |
| `crux-task` | `TaskSpec`, `ProjectTask`, `TaskManager`, `SqliteBackend`, CLI binary, LLM decomposition | **New** |
| `crux-agentic` | Pipeline step handlers (`task::create`, `task::update`, `task::list`, `task::ready`) | Extend |
| `crux-runtime` | `RegistryBackend` trait (reused, not modified) | Unchanged |

## Public API

### Types in `crux-types` (new module: `crux-types::task`)

```rust
/// Task priority levels, ordered by urgency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
         Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

/// A freeform label for categorizing tasks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskLabel(pub String);

/// The kind of relationship between two tasks.
/// Currently only `BlockedBy`; designed for extension to
/// `SubtaskOf`, `RelatedTo` without breaking changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    BlockedBy,
}
```

### Types in `crux-task` (lib)

```rust
/// A dependency edge: this task is related to `target` by `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub target: TaskId,
    pub kind: DependencyKind,
}

/// Specification for creating a task. Pure data — no ID or timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub title: String,
    pub description: Option<String>,
    pub priority: Priority,
    pub status: ProjectTaskStatus,
    pub labels: Vec<TaskLabel>,
    pub dependencies: Vec<Dependency>,
}

/// Lifecycle status of a project task.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTaskStatus {
    #[default]
    Open,
    InProgress,
    Done,
    Blocked,
    Cancelled,
}

/// A stored project task — TaskSpec + identity + timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTask {
    pub id: TaskId,
    pub spec: TaskSpec,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Filter for querying tasks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskFilter {
    pub status: Option<ProjectTaskStatus>,
    pub priority: Option<Priority>,
    pub label: Option<TaskLabel>,
}

/// Patch for updating a task. All fields optional — only set fields
/// are applied.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskPatch {
    pub status: Option<ProjectTaskStatus>,
    pub priority: Option<Priority>,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub add_labels: Vec<TaskLabel>,
    pub remove_labels: Vec<TaskLabel>,
    pub add_dependencies: Vec<Dependency>,
    pub remove_dependencies: Vec<TaskId>,
}
```

### Traits in `crux-task`

```rust
/// Port for LLM-powered goal decomposition.
/// Behind `baml` feature flag.
pub trait GoalDecomposer: Send + Sync {
    fn decompose(
        &self,
        goal: &str,
        context: Option<&str>,
    ) -> impl Future<Output = Result<Vec<TaskSpec>, TaskErr>> + Send;
}
```

### `TaskManager` in `crux-task`

```rust
/// High-level project task manager, generic over storage backend.
pub struct TaskManager<B: RegistryBackend> { .. }

impl<B: RegistryBackend> TaskManager<B> {
    pub fn new(backend: B) -> Self;

    pub async fn add(&self, spec: TaskSpec) -> Result<TaskId, TaskErr>;
    pub async fn get(&self, id: &TaskId) -> Result<ProjectTask, TaskErr>;
    pub async fn update(&self, id: &TaskId, patch: TaskPatch)
        -> Result<(), TaskErr>;
    pub async fn list(&self, filter: TaskFilter)
        -> Result<Vec<ProjectTask>, TaskErr>;
    pub async fn ready(&self) -> Result<Vec<ProjectTask>, TaskErr>;
    pub async fn blocked(&self) -> Result<Vec<ProjectTask>, TaskErr>;
    pub async fn by_priority(&self) -> Result<Vec<ProjectTask>, TaskErr>;
    pub async fn block(&self, id: &TaskId, blocker: &TaskId)
        -> Result<(), TaskErr>;
    pub async fn unblock(&self, id: &TaskId, blocker: &TaskId)
        -> Result<(), TaskErr>;
    pub async fn stats(&self) -> Result<TaskStats, TaskErr>;
}
```

### `SqliteBackend` in `crux-task`

```rust
/// Adapter: SQLite-backed RegistryBackend for crux-task.
pub struct SqliteBackend { .. }

impl SqliteBackend {
    pub fn open(path: &str) -> Result<Self, TaskErr>;
}

impl RegistryBackend for SqliteBackend { .. }
```

### Errors in `crux-task`

```rust
#[derive(Debug, thiserror::Error)]
pub enum TaskErr {
    #[error("task not found: {0}")]
    NotFound(String),
    #[error("cycle detected: adding dependency would create a cycle")]
    CycleDetected,
    #[error("storage error: {0}")]
    Storage(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("decomposition failed: {0}")]
    Decomposition(String),
}
```

### Stats type

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStats {
    pub total: usize,
    pub by_status: HashMap<ProjectTaskStatus, usize>,
    pub by_priority: HashMap<Priority, usize>,
}
```

### Handler constants in `crux-agentic::handlers`

```rust
pub const TASK_CREATE: &str = "task::create";
pub const TASK_UPDATE: &str = "task::update";
pub const TASK_LIST:   &str = "task::list";
pub const TASK_READY:  &str = "task::ready";
```

### BAML function (in `crux-task`, behind `baml` feature)

```baml
function DecomposeGoal(goal: string, context: string?) -> TaskPlan

class TaskPlan {
    tasks: TaskPlanItem[]
}

class TaskPlanItem {
    title: string
    description: string?
    priority: "p0" | "p1" | "p2" | "p3"
    labels: string[]
    blocked_by: int[]
}
```

## Data Flow

1. **CLI / Pipeline step** creates a `TaskSpec` (from args or YAML)
2. `TaskManager::add` assigns `TaskId` + timestamps, serializes to
   `ProjectTask`, writes via `RegistryBackend::put`
3. Queries (`list`, `ready`, `blocked`) read all tasks via
   `RegistryBackend::list` + `get`, filter in memory
4. **LLM decomposition** (`plan` command): goal string goes to
   `GoalDecomposer::decompose`, returns `Vec<TaskSpec>` with index-based
   deps, resolved to `TaskId`s during insertion (topological order)

## Hexagonal Boundaries

| Boundary | Port (trait) | Adapter (impl) | Location |
|---|---|---|---|
| Storage | `RegistryBackend` | `InMemoryBackend` | `crux-runtime` |
| Storage | `RegistryBackend` | `RedbBackend` | `crux-runtime` (feature `redb`) |
| Storage | `RegistryBackend` | `SqliteBackend` | `crux-task` |
| LLM | `GoalDecomposer` | `BamlDecomposer` | `crux-task` (feature `baml`) |

**ISP note:** `TaskManager` does not use `RegistryBackend::cas()` — it
overwrites via `put()`. Splitting the trait (`ReadWrite` + `Cas`) was
considered but rejected: the trait is small (4 methods), stable with 3
adapters, and the unused `cas()` cost is one no-op implementation per
backend. Revisit if more consumers with subset needs emerge.

## CLI Binary

Binary name: `crux-task`. Commands:

```
crux-task add <title> [-p P1] [-l label1,label2] [-d "desc"] [-s open]
crux-task list [--status open] [--priority P0] [--label foo] [--ready]
crux-task show <id>
crux-task update <id> [--status done] [--priority P2] [--add-label x]
crux-task block <id> --by <blocker_id>
crux-task unblock <id> --from <blocker_id>
crux-task plan "goal description" [--apply] [--context "..."]
crux-task ready
crux-task stats
```

Default backend: redb at `~/.local/share/crux-task/tasks.redb`.
`--db <path>` or `CRUX_TASK_DB` overrides. `--sqlite <path>` selects SQLite.
`--json` for machine output.

## Feature Flags (`crux-task`)

| Flag | Default | Effect |
|---|---|---|
| `sqlite` | no | Enables `SqliteBackend` (dep: `rusqlite`) |
| `redb` | yes | Enables `RedbBackend` (dep: `redb`) |
| `baml` | no | Enables LLM decomposition via `GoalDecomposer` |

## Out of Scope

- Doob compatibility or migration tooling
- GitHub/Linear issue sync
- Kanban TUI (can be added later over `TaskManager` queries)
- Hierarchical task trees (`SubtaskOf` edge) — designed for but not
  implemented in v1
- Handoff file integration

## Risk

- [x] Breaking API changes: **no** — no existing crate APIs are modified
- [x] New external dependency: **yes** — `rusqlite` in `crux-task` (behind
  `sqlite` feature flag, bundled). `redb` already a workspace dep.
- [x] Feature flag required: **yes** — `sqlite` and `baml` on `crux-task`
- [ ] New dep edge: `crux-agentic` → `crux-task` (for pipeline handlers)
