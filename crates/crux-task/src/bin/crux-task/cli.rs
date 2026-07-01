use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "crux-task", about = "Project task management for crux")]
pub struct Cli {
    /// Output in JSON format
    #[arg(long, global = true)]
    pub json: bool,

    /// Database path (redb). Overrides CRUX_TASK_DB env var.
    #[arg(long, global = true)]
    pub db: Option<String>,

    /// Use SQLite backend at this path instead of redb.
    #[arg(long, global = true)]
    pub sqlite: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Add a new task
    Add {
        /// Task title
        title: String,
        /// Priority (p0, p1, p2, p3)
        #[arg(short, long, default_value = "p2")]
        priority: PriorityCli,
        /// Labels (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        labels: Vec<String>,
        /// Description
        #[arg(short, long)]
        description: Option<String>,
        /// Status (open, in_progress, done, blocked, cancelled)
        #[arg(short, long, default_value = "open")]
        status: StatusCli,
    },
    /// List tasks
    List {
        #[arg(long)]
        status: Option<StatusCli>,
        #[arg(long)]
        priority: Option<PriorityCli>,
        #[arg(long)]
        label: Option<String>,
        /// Show only ready tasks (unblocked and open)
        #[arg(long)]
        ready: bool,
    },
    /// Show a task by ID
    Show { id: String },
    /// Update a task
    Update {
        id: String,
        #[arg(long)]
        status: Option<StatusCli>,
        #[arg(long)]
        priority: Option<PriorityCli>,
        #[arg(long)]
        add_label: Vec<String>,
        #[arg(long)]
        rm_label: Vec<String>,
    },
    /// Block a task by another
    Block {
        id: String,
        /// The blocker task ID
        #[arg(long)]
        by: String,
    },
    /// Unblock a task
    Unblock {
        id: String,
        /// The blocker to remove
        #[arg(long)]
        from: String,
    },
    /// Show ready tasks (shortcut for list --ready)
    Ready,
    /// Show task statistics
    Stats,
}

#[derive(Clone, ValueEnum)]
pub enum PriorityCli {
    P0,
    P1,
    P2,
    P3,
}

#[derive(Clone, ValueEnum)]
pub enum StatusCli {
    Open,
    InProgress,
    Done,
    Blocked,
    Cancelled,
}
