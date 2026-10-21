//! `crux resume` — continue a paused or failed pipeline run from a checkpoint.
//!
//! Loads the [`RunState`] from the checkpoint file, then replays the saved
//! trace so previously completed steps are skipped and execution continues
//! from where it left off.

use std::path::Path;

use crate::checkpoint::{RunStatus, load_checkpoint, save_checkpoint};
use crate::run::{RunConfig, cmd_run_dispatch};

/// Resume a paused or failed run from the checkpoint at `checkpoint_path`.
///
/// Exits the process with a non-zero code on any unrecoverable error.
pub fn cmd_resume(checkpoint_path: &str, replay_mode: &str) {
    let ckpt_path = Path::new(checkpoint_path);
    let mut state = load_checkpoint(ckpt_path).unwrap_or_else(|e| {
        eprintln!("error: cannot load checkpoint {checkpoint_path}: {e}");
        std::process::exit(1);
    });

    match state.status {
        RunStatus::Complete => {
            eprintln!(
                "[crux] run {} is already complete — nothing to resume",
                state.run_id
            );
            return;
        }
        RunStatus::Running => {
            eprintln!(
                "error: run {} is marked as running; another process may be active",
                state.run_id
            );
            std::process::exit(1);
        }
        RunStatus::Paused | RunStatus::Failed => {}
    }

    let trace_path_str = state
        .trace_path
        .to_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            eprintln!("error: checkpoint trace path is not valid UTF-8");
            std::process::exit(1);
        });

    if !state.trace_path.is_file() {
        eprintln!(
            "error: checkpoint trace not found at {}; cannot resume",
            trace_path_str
        );
        std::process::exit(1);
    }

    let events_jsonl_path_str: Option<String> = state
        .events_jsonl_path
        .as_ref()
        .and_then(|p| p.to_str().map(str::to_string));

    eprintln!(
        "[crux] resuming run {} (status: {})",
        state.run_id, state.status
    );

    // Mark as running before dispatching so concurrent resumes see the state.
    state.status = RunStatus::Running;
    let _ = save_checkpoint(&state, ckpt_path); // best-effort

    cmd_run_dispatch(&RunConfig {
        pipeline_arg: Some(state.pipeline_path.as_str()),
        target_or_input: None,
        check: false,
        target_flag: None,
        input_flag: state.input_path.as_deref(),
        plugins_path: state.plugins_path.as_deref(),
        quiet: false,
        summary: true,
        json: false,
        verbose: false,
        dry_run: false,
        replay_path: Some(trace_path_str.as_str()),
        replay_mode_str: replay_mode,
        save_trace_path: None,
        events_jsonl_path: events_jsonl_path_str.as_deref(),
        through_step: None,
        strict: false,
        checkpoint_path: None, // resume does not re-checkpoint by default
    });
}
