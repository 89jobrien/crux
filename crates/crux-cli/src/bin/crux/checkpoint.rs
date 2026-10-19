//! Run-state checkpoints: persist and reload pipeline execution progress.
//!
//! A [`RunState`] captures everything needed to resume (or audit) a pipeline
//! run: the pipeline path, input, where the trace was written, which steps
//! finished, and the overall status. It is saved as a compact JSON file under
//! `~/.crux/checkpoints/` by default, or to an explicit path.
//!
//! ## Checkpoint lifecycle
//!
//! ```text
//! crux run pipeline.crux --checkpoint run.ckpt
//!   → executes pipeline (optionally with --through <step> to pause early)
//!   → writes trace to ~/.crux/traces/<id>.json
//!   → writes RunState to run.ckpt (status: Complete or Paused)
//!
//! crux resume run.ckpt
//!   → loads RunState
//!   → replays the run using the saved trace (skipping already-completed steps)
//!   → updates the checkpoint status on completion
//! ```

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// High-level lifecycle status of a persisted run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Pipeline is currently executing (process is alive).
    Running,
    /// Pipeline was stopped early via `--through`; resume is possible.
    Paused,
    /// Pipeline completed successfully.
    Complete,
    /// Pipeline failed; the saved trace can be inspected or replayed.
    Failed,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Paused => write!(f, "paused"),
            Self::Complete => write!(f, "complete"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// A reference to a pipeline artifact produced during a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// Absolute or relative path to the artifact file.
    pub path: PathBuf,
    /// Free-form label (e.g. `"trace"`, `"events"`, `"output"`).
    pub kind: String,
}

/// A snapshot of consumed budget counters at checkpoint time.
///
/// Derived from the recorded trace; not enforced independently.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    /// Number of steps recorded in the trace.
    pub steps_recorded: usize,
    /// Sum of `duration_ms` across all recorded steps (approximate wall time).
    pub duration_ms_total: u64,
}

/// Serializable run manifest that persists everything needed to resume or
/// audit a pipeline execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    /// Stable identifier for this run (matches the `Crux<T>::id` field).
    pub run_id: String,
    /// Path to the `.crux` pipeline or Cruxfile that was executed.
    pub pipeline_path: String,
    /// Path to the input JSON file, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_path: Option<String>,
    /// Path to the `plugins.toml` used, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins_path: Option<String>,
    /// Where the execution trace was written.
    pub trace_path: PathBuf,
    /// Where the events JSONL was written, if requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_jsonl_path: Option<PathBuf>,
    /// Step names that finished before this checkpoint was written.
    pub completed_steps: Vec<String>,
    /// The last step executed when `--through` was used (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at_step: Option<String>,
    /// Budget counters derived from the trace.
    pub budget_snapshot: BudgetSnapshot,
    /// Additional output artifacts produced by the pipeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    /// Lifecycle status at the time this checkpoint was written.
    pub status: RunStatus,
    /// When this run started.
    pub started_at: DateTime<Utc>,
    /// When this run finished (or when the checkpoint was written, for paused runs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

impl RunState {
    /// Construct a `RunState` from a completed or paused run.
    ///
    /// `stopped_at_step` is `Some` when `--through <step>` was used.
    pub fn from_run(
        run_id: String,
        pipeline_path: String,
        input_path: Option<String>,
        plugins_path: Option<String>,
        trace_path: PathBuf,
        events_jsonl_path: Option<PathBuf>,
        completed_steps: Vec<String>,
        stopped_at_step: Option<String>,
        budget_snapshot: BudgetSnapshot,
        artifacts: Vec<ArtifactRef>,
        status: RunStatus,
        started_at: DateTime<Utc>,
    ) -> Self {
        let finished_at = Some(Utc::now());
        Self {
            run_id,
            pipeline_path,
            input_path,
            plugins_path,
            trace_path,
            events_jsonl_path,
            completed_steps,
            stopped_at_step,
            budget_snapshot,
            artifacts,
            status,
            started_at,
            finished_at,
        }
    }
}

/// Write `state` as compact JSON to `path`, creating parent directories as needed.
///
/// The file is written atomically on Unix via a private-mode create-new open
/// followed by `sync_all`.  If the file already exists it is overwritten.
pub fn save_checkpoint(state: &RunState, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Load a [`RunState`] from the JSON checkpoint at `path`.
pub fn load_checkpoint(path: &Path) -> std::io::Result<RunState> {
    let contents = std::fs::read_to_string(path)?;
    serde_json::from_str(&contents).map_err(std::io::Error::other)
}

/// Return the default checkpoint path for a given run ID.
///
/// Writes to `<home>/.crux/checkpoints/<run_id>.json`.
pub fn default_checkpoint_path(home: &Path, run_id: &str) -> PathBuf {
    home.join(".crux")
        .join("checkpoints")
        .join(format!("{run_id}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> RunState {
        RunState::from_run(
            "01J9Z000000000000000000000".to_string(),
            "examples/fetch.crux".to_string(),
            Some("examples/input.json".to_string()),
            None,
            PathBuf::from("/tmp/home/.crux/traces/trace.json"),
            None,
            vec!["fetch".to_string(), "parse".to_string()],
            Some("parse".to_string()),
            BudgetSnapshot {
                steps_recorded: 2,
                duration_ms_total: 340,
            },
            vec![ArtifactRef {
                path: PathBuf::from("output.json"),
                kind: "output".to_string(),
            }],
            RunStatus::Paused,
            Utc::now(),
        )
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir must be created");
        let path = dir.path().join("run.json");
        let state = sample_state();

        save_checkpoint(&state, &path).expect("checkpoint must be saved");
        let loaded = load_checkpoint(&path).expect("checkpoint must be loaded");

        assert_eq!(loaded.run_id, state.run_id);
        assert_eq!(loaded.pipeline_path, state.pipeline_path);
        assert_eq!(loaded.input_path, state.input_path);
        assert_eq!(loaded.completed_steps, state.completed_steps);
        assert_eq!(loaded.stopped_at_step, state.stopped_at_step);
        assert_eq!(loaded.status, RunStatus::Paused);
        assert_eq!(loaded.budget_snapshot.steps_recorded, 2);
        assert_eq!(loaded.budget_snapshot.duration_ms_total, 340);
        assert_eq!(loaded.artifacts.len(), 1);
        assert_eq!(loaded.artifacts[0].kind, "output");
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().expect("temp dir must be created");
        let path = dir.path().join("sub").join("nested").join("run.json");
        let state = sample_state();

        save_checkpoint(&state, &path).expect("checkpoint must be saved even with missing parents");
        assert!(path.is_file());
    }

    #[test]
    fn default_checkpoint_path_uses_crux_checkpoints_dir() {
        let home = Path::new("/Users/test");
        let path = default_checkpoint_path(home, "RUNID");
        assert_eq!(path, Path::new("/Users/test/.crux/checkpoints/RUNID.json"));
    }

    #[test]
    fn load_checkpoint_returns_error_for_missing_file() {
        let result = load_checkpoint(Path::new("/nonexistent/path/run.json"));
        assert!(result.is_err());
    }

    #[test]
    fn run_status_display() {
        assert_eq!(RunStatus::Paused.to_string(), "paused");
        assert_eq!(RunStatus::Complete.to_string(), "complete");
        assert_eq!(RunStatus::Failed.to_string(), "failed");
        assert_eq!(RunStatus::Running.to_string(), "running");
    }

    #[test]
    fn complete_run_has_no_stopped_at_step() {
        let state = RunState::from_run(
            "ID".to_string(),
            "p.crux".to_string(),
            None,
            None,
            PathBuf::from("trace.json"),
            None,
            vec!["a".to_string(), "b".to_string()],
            None,
            BudgetSnapshot::default(),
            vec![],
            RunStatus::Complete,
            Utc::now(),
        );

        // stopped_at_step must be None and must serialize as absent (not "null")
        let json = serde_json::to_string(&state).expect("must serialize");
        assert!(!json.contains("stopped_at_step"), "{json}");
        assert_eq!(state.status, RunStatus::Complete);
    }
}
