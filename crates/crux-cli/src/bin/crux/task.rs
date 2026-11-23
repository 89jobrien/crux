//! `crux task` — project task management subcommands.
//!
//! Delegates to `crux_task::TaskManager` with a persistent redb backend
//! (path resolved from `--db` flag, `CRUX_TASK_DB` env var, or the XDG
//! data directory).  `crux-task`'s default features enable redb, so the
//! backend is always available.

use std::fmt::Write as _;

use clap::{Subcommand, ValueEnum};
use crux_task::{ProjectTask, ProjectTaskStatus, TaskFilter, TaskManager, TaskPatch, TaskSpec};
use crux_types::id::TaskId;
use crux_types::task::{Priority, TaskLabel};

// ── Status / Priority adapters ────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum)]
pub enum StatusArg {
    Open,
    InProgress,
    Done,
    Blocked,
    Cancelled,
}

impl From<StatusArg> for ProjectTaskStatus {
    fn from(s: StatusArg) -> Self {
        match s {
            StatusArg::Open => Self::Open,
            StatusArg::InProgress => Self::InProgress,
            StatusArg::Done => Self::Done,
            StatusArg::Blocked => Self::Blocked,
            StatusArg::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
pub enum PriorityArg {
    P0,
    P1,
    P2,
    P3,
}

impl From<PriorityArg> for Priority {
    fn from(p: PriorityArg) -> Self {
        match p {
            PriorityArg::P0 => Self::P0,
            PriorityArg::P1 => Self::P1,
            PriorityArg::P2 => Self::P2,
            PriorityArg::P3 => Self::P3,
        }
    }
}

// ── Subcommand definitions ─────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum TaskCommand {
    /// Create a new project task
    Create {
        /// Task title
        name: String,
        /// Optional description
        #[arg(short, long)]
        description: Option<String>,
        /// Priority (p0–p3)
        #[arg(short, long, default_value = "p2")]
        priority: PriorityArg,
        /// Initial status
        #[arg(short, long, default_value = "open")]
        status: StatusArg,
    },
    /// List tasks, optionally filtered by status and/or priority
    List {
        /// Filter by status
        #[arg(long)]
        status: Option<StatusArg>,
        /// Filter by priority
        #[arg(long)]
        priority: Option<PriorityArg>,
    },
    /// Update task status and/or priority
    Update {
        /// Task ID
        id: String,
        /// New status
        #[arg(long)]
        status: Option<StatusArg>,
        /// New priority
        #[arg(long)]
        priority: Option<PriorityArg>,
    },
    /// Show details for a single task
    Get {
        /// Task ID
        id: String,
    },
    /// List open or in-progress tasks whose dependencies are all done
    Ready,
    /// List tasks with at least one unresolved dependency
    Blocked,
    /// Record that a task is blocked by another task
    Block {
        /// Task that is blocked
        id: String,
        /// Task that must finish first
        blocker: String,
    },
    /// Remove a blocking dependency from a task
    Unblock {
        /// Task that was blocked
        id: String,
        /// Blocking task to remove
        blocker: String,
    },
    /// Show task counts by status and priority
    Stats,
}

// ── Dispatch ─────────────────────────────────────────────────────────────────

/// Resolve the redb database path from an explicit flag or environment.
fn resolve_db_path(db_flag: Option<&str>) -> String {
    if let Some(path) = db_flag {
        return path.to_owned();
    }
    std::env::var("CRUX_TASK_DB").unwrap_or_else(|_| {
        let dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("crux-task");
        std::fs::create_dir_all(&dir).ok();
        dir.join("tasks.redb").to_string_lossy().into_owned()
    })
}

pub fn cmd_task(command: TaskCommand, db_flag: Option<&str>) {
    let db_path = resolve_db_path(db_flag);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    if let Err(e) = rt.block_on(run_task(command, &db_path)) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run_task(command: TaskCommand, db_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // crux-task's default features enable the redb backend, so RedbBackend is
    // always available here.  The path is resolved by `resolve_db_path` above.
    let backend = crux_runtime::registry::RedbBackend::open(db_path)?;
    let mgr = TaskManager::new(backend);
    let output = execute(command, &mgr).await?;
    print!("{output}");
    Ok(())
}

/// Runs one task command against a manager and returns the text to print.
async fn execute<B: crux_runtime::registry::RegistryBackend>(
    command: TaskCommand,
    mgr: &TaskManager<B>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();
    match command {
        TaskCommand::Create {
            name,
            description,
            priority,
            status,
        } => {
            let spec = TaskSpec {
                title: name,
                description,
                priority: priority.into(),
                status: status.into(),
                labels: vec![],
                dependencies: vec![],
            };
            let id = mgr.add(spec).await?;
            let _ = writeln!(out, "Created: {id}");
        }

        TaskCommand::List { status, priority } => {
            let filter = TaskFilter {
                status: status.map(Into::into),
                priority: priority.map(Into::into),
                ..Default::default()
            };
            let tasks = mgr.list(filter).await?;
            push_task_list(&mut out, &tasks, "No tasks found.");
        }

        TaskCommand::Update {
            id,
            status,
            priority,
        } => {
            let tid: TaskId = id.parse()?;
            let patch = TaskPatch {
                status: status.map(Into::into),
                priority: priority.map(Into::into),
                ..Default::default()
            };
            mgr.update(&tid, patch).await?;
            let _ = writeln!(out, "Updated: {tid}");
        }

        TaskCommand::Get { id } => {
            let tid: TaskId = id.parse()?;
            let t = mgr.get(&tid).await?;
            push_task_detail(&mut out, &t);
        }

        TaskCommand::Ready => {
            let tasks = mgr.ready().await?;
            push_task_list(&mut out, &tasks, "No ready tasks.");
        }

        TaskCommand::Blocked => {
            let tasks = mgr.blocked().await?;
            push_task_list(&mut out, &tasks, "No blocked tasks.");
        }

        TaskCommand::Block { id, blocker } => {
            let tid: TaskId = id.parse()?;
            let bid: TaskId = blocker.parse()?;
            mgr.block(&tid, &bid).await?;
            let _ = writeln!(out, "Blocked: {tid} by {bid}");
        }

        TaskCommand::Unblock { id, blocker } => {
            let tid: TaskId = id.parse()?;
            let bid: TaskId = blocker.parse()?;
            mgr.unblock(&tid, &bid).await?;
            let _ = writeln!(out, "Unblocked: {tid} from {bid}");
        }

        TaskCommand::Stats => {
            let stats = mgr.stats().await?;
            push_stats(&mut out, &stats);
        }
    }
    Ok(out)
}

fn push_task_list(out: &mut String, tasks: &[ProjectTask], empty_msg: &str) {
    if tasks.is_empty() {
        let _ = writeln!(out, "{empty_msg}");
        return;
    }
    for t in tasks {
        let _ = writeln!(
            out,
            "{} [{:?}] {} ({:?})",
            t.id, t.spec.priority, t.spec.title, t.spec.status,
        );
    }
}

fn push_task_detail(out: &mut String, t: &ProjectTask) {
    let _ = writeln!(out, "ID:       {}", t.id);
    let _ = writeln!(out, "Title:    {}", t.spec.title);
    if let Some(ref d) = t.spec.description {
        let _ = writeln!(out, "Desc:     {d}");
    }
    let _ = writeln!(out, "Priority: {:?}", t.spec.priority);
    let _ = writeln!(out, "Status:   {:?}", t.spec.status);
    if !t.spec.labels.is_empty() {
        let labels: Vec<&str> = t
            .spec
            .labels
            .iter()
            .map(|l: &TaskLabel| l.0.as_str())
            .collect();
        let _ = writeln!(out, "Labels:   {}", labels.join(", "));
    }
    let _ = writeln!(out, "Created:  {}", t.created_at);
    let _ = writeln!(out, "Updated:  {}", t.updated_at);
}

fn push_stats(out: &mut String, stats: &crux_task::TaskStats) {
    let _ = writeln!(out, "Total: {}", stats.total);
    let statuses = [
        ProjectTaskStatus::Open,
        ProjectTaskStatus::InProgress,
        ProjectTaskStatus::Done,
        ProjectTaskStatus::Blocked,
        ProjectTaskStatus::Cancelled,
    ];
    for s in &statuses {
        let count = stats.by_status.get(s).copied().unwrap_or(0);
        let _ = writeln!(out, "  {s:?}: {count}");
    }
    let priorities = [Priority::P0, Priority::P1, Priority::P2, Priority::P3];
    for p in &priorities {
        let count = stats.by_priority.get(p).copied().unwrap_or(0);
        let _ = writeln!(out, "  {p:?}: {count}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use crux_runtime::registry::InMemoryBackend;
    use std::future::Future;

    /// Minimal harness so we can parse `crux task <args>` in tests.
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TaskCommand,
    }

    fn block_on<F: Future>(fut: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(fut)
    }

    fn new_mgr() -> TaskManager<InMemoryBackend> {
        TaskManager::new(InMemoryBackend::new())
    }

    fn create_cmd(name: &str, priority: PriorityArg) -> TaskCommand {
        TaskCommand::Create {
            name: name.to_owned(),
            description: None,
            priority,
            status: StatusArg::Open,
        }
    }

    /// Creates a task through the command layer and returns its ID string.
    async fn create_task(mgr: &TaskManager<InMemoryBackend>, name: &str) -> String {
        let out = execute(create_cmd(name, PriorityArg::P2), mgr)
            .await
            .expect("create succeeds");
        out.trim()
            .strip_prefix("Created: ")
            .expect("create output has Created prefix")
            .to_owned()
    }

    async fn run(mgr: &TaskManager<InMemoryBackend>, cmd: TaskCommand) -> String {
        execute(cmd, mgr).await.expect("command succeeds")
    }

    #[test]
    fn parse_create() {
        let cli = TestCli::try_parse_from(["crux-task", "create", "my task"]);
        assert!(matches!(
            cli,
            Ok(TestCli {
                command: TaskCommand::Create { .. }
            })
        ));
    }

    #[test]
    fn parse_list_with_status() {
        let cli = TestCli::try_parse_from(["crux-task", "list", "--status", "done"]);
        assert!(matches!(
            cli,
            Ok(TestCli {
                command: TaskCommand::List {
                    status: Some(StatusArg::Done),
                    priority: None,
                }
            })
        ));
    }

    #[test]
    fn parse_list_no_filter() {
        let cli = TestCli::try_parse_from(["crux-task", "list"]);
        assert!(matches!(
            cli,
            Ok(TestCli {
                command: TaskCommand::List {
                    status: None,
                    priority: None,
                }
            })
        ));
    }

    #[test]
    fn parse_list_with_priority() {
        let cli = TestCli::try_parse_from(["crux-task", "list", "--priority", "p0"]);
        assert!(matches!(
            cli,
            Ok(TestCli {
                command: TaskCommand::List {
                    priority: Some(PriorityArg::P0),
                    ..
                }
            })
        ));
    }

    #[test]
    fn parse_update_status() {
        let cli =
            TestCli::try_parse_from(["crux-task", "update", "some-id", "--status", "in-progress"]);
        assert!(matches!(
            cli,
            Ok(TestCli {
                command: TaskCommand::Update {
                    status: Some(StatusArg::InProgress),
                    ..
                }
            })
        ));
    }

    #[test]
    fn parse_get() {
        let cli = TestCli::try_parse_from(["crux-task", "get", "abc123"]);
        assert!(matches!(
            cli,
            Ok(TestCli {
                command: TaskCommand::Get { .. }
            })
        ));
    }

    #[test]
    fn parse_ready_blocked_stats() {
        assert!(matches!(
            TestCli::try_parse_from(["crux-task", "ready"]),
            Ok(TestCli {
                command: TaskCommand::Ready
            })
        ));
        assert!(matches!(
            TestCli::try_parse_from(["crux-task", "blocked"]),
            Ok(TestCli {
                command: TaskCommand::Blocked
            })
        ));
        assert!(matches!(
            TestCli::try_parse_from(["crux-task", "stats"]),
            Ok(TestCli {
                command: TaskCommand::Stats
            })
        ));
    }

    #[test]
    fn parse_block_and_unblock_take_two_ids() {
        let cli = TestCli::try_parse_from(["crux-task", "block", "a", "b"]);
        assert!(matches!(
            cli,
            Ok(TestCli {
                command: TaskCommand::Block { .. }
            })
        ));
        let cli = TestCli::try_parse_from(["crux-task", "unblock", "a", "b"]);
        assert!(matches!(
            cli,
            Ok(TestCli {
                command: TaskCommand::Unblock { .. }
            })
        ));
        assert!(TestCli::try_parse_from(["crux-task", "block", "a"]).is_err());
    }

    #[test]
    fn ready_excludes_task_blocked_by_open_task() {
        block_on(async {
            let mgr = new_mgr();
            let a = create_task(&mgr, "first").await;
            let b = create_task(&mgr, "second").await;
            run(
                &mgr,
                TaskCommand::Block {
                    id: b.clone(),
                    blocker: a.clone(),
                },
            )
            .await;

            let ready = run(&mgr, TaskCommand::Ready).await;
            assert!(ready.contains(&a), "blocker should be ready: {ready}");
            assert!(
                !ready.contains(&b),
                "blocked task must not be ready: {ready}"
            );

            let blocked = run(&mgr, TaskCommand::Blocked).await;
            assert!(
                blocked.contains(&b),
                "task should be listed as blocked: {blocked}"
            );
            assert!(
                !blocked.contains(&a),
                "blocker must not be blocked: {blocked}"
            );
        });
    }

    #[test]
    fn unblock_restores_readiness() {
        block_on(async {
            let mgr = new_mgr();
            let a = create_task(&mgr, "first").await;
            let b = create_task(&mgr, "second").await;
            run(
                &mgr,
                TaskCommand::Block {
                    id: b.clone(),
                    blocker: a.clone(),
                },
            )
            .await;
            let out = run(
                &mgr,
                TaskCommand::Unblock {
                    id: b.clone(),
                    blocker: a.clone(),
                },
            )
            .await;
            assert!(out.starts_with("Unblocked: "), "unexpected output: {out}");

            let ready = run(&mgr, TaskCommand::Ready).await;
            assert!(
                ready.contains(&b),
                "unblocked task should be ready: {ready}"
            );
        });
    }

    #[test]
    fn list_filters_by_priority() {
        block_on(async {
            let mgr = new_mgr();
            run(&mgr, create_cmd("urgent", PriorityArg::P0)).await;
            run(&mgr, create_cmd("routine", PriorityArg::P2)).await;

            let out = run(
                &mgr,
                TaskCommand::List {
                    status: None,
                    priority: Some(PriorityArg::P0),
                },
            )
            .await;
            assert!(out.contains("urgent"), "p0 task missing: {out}");
            assert!(
                !out.contains("routine"),
                "p2 task leaked through filter: {out}"
            );
        });
    }

    #[test]
    fn stats_reports_totals_by_status_and_priority() {
        block_on(async {
            let mgr = new_mgr();
            run(&mgr, create_cmd("one", PriorityArg::P0)).await;
            run(&mgr, create_cmd("two", PriorityArg::P0)).await;
            run(&mgr, create_cmd("three", PriorityArg::P3)).await;

            let out = run(&mgr, TaskCommand::Stats).await;
            assert!(out.contains("Total: 3"), "missing total: {out}");
            assert!(out.contains("Open: 3"), "missing open count: {out}");
            assert!(out.contains("P0: 2"), "missing p0 count: {out}");
            assert!(out.contains("P3: 1"), "missing p3 count: {out}");
        });
    }
}
