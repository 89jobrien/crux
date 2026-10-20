//! `crux task` — project task management subcommands.
//!
//! Delegates to `crux_task::TaskManager` with a persistent redb backend
//! (path resolved from `--db` flag, `CRUX_TASK_DB` env var, or the XDG
//! data directory).  `crux-task`'s default features enable redb, so the
//! backend is always available.

use clap::{Subcommand, ValueEnum};
use crux_task::{ProjectTaskStatus, TaskFilter, TaskManager, TaskPatch, TaskSpec};
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
    /// List tasks, optionally filtered by status
    List {
        /// Filter by status
        #[arg(long)]
        status: Option<StatusArg>,
    },
    /// Update task status (and optionally priority)
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
    dispatch(command, TaskManager::new(backend)).await
}

async fn dispatch<B: crux_runtime::registry::RegistryBackend>(
    command: TaskCommand,
    mgr: TaskManager<B>,
) -> Result<(), Box<dyn std::error::Error>> {
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
            println!("Created: {id}");
        }

        TaskCommand::List { status } => {
            let filter = TaskFilter {
                status: status.map(Into::into),
                ..Default::default()
            };
            let tasks = mgr.list(filter).await?;
            if tasks.is_empty() {
                println!("No tasks found.");
            } else {
                for t in &tasks {
                    println!(
                        "{} [{:?}] {} ({:?})",
                        t.id, t.spec.priority, t.spec.title, t.spec.status,
                    );
                }
            }
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
            println!("Updated: {tid}");
        }

        TaskCommand::Get { id } => {
            let tid: TaskId = id.parse()?;
            let t = mgr.get(&tid).await?;
            println!("ID:       {}", t.id);
            println!("Title:    {}", t.spec.title);
            if let Some(ref d) = t.spec.description {
                println!("Desc:     {d}");
            }
            println!("Priority: {:?}", t.spec.priority);
            println!("Status:   {:?}", t.spec.status);
            if !t.spec.labels.is_empty() {
                let labels: Vec<&str> = t
                    .spec
                    .labels
                    .iter()
                    .map(|l: &TaskLabel| l.0.as_str())
                    .collect();
                println!("Labels:   {}", labels.join(", "));
            }
            println!("Created:  {}", t.created_at);
            println!("Updated:  {}", t.updated_at);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Minimal harness so we can parse `crux task <args>` in tests.
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TaskCommand,
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
                    status: Some(StatusArg::Done)
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
                command: TaskCommand::List { status: None }
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
}
