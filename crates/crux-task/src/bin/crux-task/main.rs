mod cli;

use clap::Parser;
use cli::{Cli, Command, PriorityCli, StatusCli};
use crux_runtime::registry::InMemoryBackend;
use crux_runtime::registry::RegistryBackend;
use crux_task::{ProjectTaskStatus, TaskFilter, TaskManager, TaskPatch, TaskSpec};
use crux_types::id::TaskId;
use crux_types::task::{Priority, TaskLabel};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn to_priority(p: PriorityCli) -> Priority {
    match p {
        PriorityCli::P0 => Priority::P0,
        PriorityCli::P1 => Priority::P1,
        PriorityCli::P2 => Priority::P2,
        PriorityCli::P3 => Priority::P3,
    }
}

fn to_status(s: StatusCli) -> ProjectTaskStatus {
    match s {
        StatusCli::Open => ProjectTaskStatus::Open,
        StatusCli::InProgress => ProjectTaskStatus::InProgress,
        StatusCli::Done => ProjectTaskStatus::Done,
        StatusCli::Blocked => ProjectTaskStatus::Blocked,
        StatusCli::Cancelled => ProjectTaskStatus::Cancelled,
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "sqlite")]
    if let Some(ref path) = cli.sqlite {
        let backend = crux_task::sqlite::SqliteBackend::open(path)?;
        return dispatch(cli, TaskManager::new(backend)).await;
    }

    #[cfg(feature = "redb")]
    {
        let db_path = cli.db.clone().unwrap_or_else(|| {
            std::env::var("CRUX_TASK_DB").unwrap_or_else(|_| {
                let dir = dirs::data_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("crux-task");
                std::fs::create_dir_all(&dir).ok();
                dir.join("tasks.redb").to_string_lossy().to_string()
            })
        });
        let backend = crux_runtime::registry::RedbBackend::open(&db_path)?;
        return dispatch(cli, TaskManager::new(backend)).await;
    }

    #[allow(unreachable_code)]
    dispatch(cli, TaskManager::new(InMemoryBackend::new())).await
}

async fn dispatch<B: RegistryBackend>(
    cli: Cli,
    mgr: TaskManager<B>,
) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Add {
            title,
            priority,
            labels,
            description,
            status,
        } => {
            let spec = TaskSpec {
                title,
                description,
                priority: to_priority(priority),
                status: to_status(status),
                labels: labels.into_iter().map(TaskLabel).collect(),
                dependencies: vec![],
            };
            let id = mgr.add(spec).await?;
            if cli.json {
                println!("{}", serde_json::json!({ "id": id.as_str() }));
            } else {
                println!("Created: {id}");
            }
        }
        Command::List {
            status,
            priority,
            label,
            ready,
        } => {
            let tasks = if ready {
                mgr.ready().await?
            } else {
                let filter = TaskFilter {
                    status: status.map(to_status),
                    priority: priority.map(to_priority),
                    label: label.map(TaskLabel),
                };
                mgr.list(filter).await?
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&tasks)?);
            } else {
                for t in &tasks {
                    println!(
                        "{} [{}] {} ({})",
                        t.id,
                        serde_json::to_value(t.spec.priority)?
                            .as_str()
                            .unwrap_or("?"),
                        t.spec.title,
                        serde_json::to_value(&t.spec.status)?
                            .as_str()
                            .unwrap_or("?"),
                    );
                }
                if tasks.is_empty() {
                    println!("No tasks found.");
                }
            }
        }
        Command::Show { id } => {
            let tid: TaskId = id.parse()?;
            let task = mgr.get(&tid).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&task)?);
            } else {
                println!("ID:       {}", task.id);
                println!("Title:    {}", task.spec.title);
                if let Some(ref d) = task.spec.description {
                    println!("Desc:     {d}");
                }
                println!("Priority: {:?}", task.spec.priority);
                println!("Status:   {:?}", task.spec.status);
                if !task.spec.labels.is_empty() {
                    let labels: Vec<&str> = task.spec.labels.iter().map(|l| l.0.as_str()).collect();
                    println!("Labels:   {}", labels.join(", "));
                }
                if !task.spec.dependencies.is_empty() {
                    println!("Blocked by:");
                    for dep in &task.spec.dependencies {
                        println!("  - {}", dep.target);
                    }
                }
                println!("Created:  {}", task.created_at);
                println!("Updated:  {}", task.updated_at);
            }
        }
        Command::Update {
            id,
            status,
            priority,
            add_label,
            rm_label,
        } => {
            let tid: TaskId = id.parse()?;
            let patch = TaskPatch {
                status: status.map(to_status),
                priority: priority.map(to_priority),
                add_labels: add_label.into_iter().map(TaskLabel).collect(),
                remove_labels: rm_label.into_iter().map(TaskLabel).collect(),
                ..Default::default()
            };
            mgr.update(&tid, patch).await?;
            println!("Updated: {tid}");
        }
        Command::Block { id, by } => {
            let tid: TaskId = id.parse()?;
            let blocker: TaskId = by.parse()?;
            mgr.block(&tid, &blocker).await?;
            println!("Blocked: {tid} by {blocker}");
        }
        Command::Unblock { id, from } => {
            let tid: TaskId = id.parse()?;
            let blocker: TaskId = from.parse()?;
            mgr.unblock(&tid, &blocker).await?;
            println!("Unblocked: {tid} from {blocker}");
        }
        Command::Ready => {
            let tasks = mgr.ready().await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&tasks)?);
            } else {
                for t in &tasks {
                    println!(
                        "{} [{}] {}",
                        t.id,
                        serde_json::to_value(t.spec.priority)?
                            .as_str()
                            .unwrap_or("?"),
                        t.spec.title,
                    );
                }
                if tasks.is_empty() {
                    println!("No ready tasks.");
                }
            }
        }
        Command::Stats => {
            let s = mgr.stats().await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                println!("Total: {}", s.total);
                println!("By status:");
                for (k, v) in &s.by_status {
                    println!("  {k:?}: {v}");
                }
                println!("By priority:");
                for (k, v) in &s.by_priority {
                    println!("  {k:?}: {v}");
                }
            }
        }
    }
    Ok(())
}
