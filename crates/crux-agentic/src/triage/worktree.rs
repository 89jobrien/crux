//! Handlers for orphaned worktree detection and branch cleanup planning.

use crux_script::{HandlerMetadata, HandlerOutput, HandlerRegistry, RiskLevel};
use serde_json::{Value, json};

use crate::handlers;

// Branch cleanup confidence thresholds
const CONFIDENCE_ORPHANED_WORKTREES: f32 = 0.3;
const MANY_BRANCHES_THRESHOLD: usize = 5;
const CONFIDENCE_MANY_BRANCHES: f32 = 0.6;
const CONFIDENCE_CLEAN_STATE: f32 = 0.9;

pub(super) fn register(registry: &mut HandlerRegistry) {
    register_detect_orphaned_worktrees(registry);
    register_build_cleanup_plan(registry);
}

fn register_detect_orphaned_worktrees(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_DETECT_ORPHANED_WORKTREES)
            .describe("Identify worktrees not on main or develop from git worktree list output.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let worktree_text = input
                .pointer("/worktree_list/output")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // git worktree list output: "<path>  <sha>  [<branch>]" or "(bare)"
            let orphans: Vec<Value> = worktree_text
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.contains("(bare)"))
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    let path = parts.first()?;
                    let branch = parts.get(2).map(|b| b.trim_matches(['[', ']']));
                    // Heuristic: worktrees not on main/develop are candidates
                    let is_main = branch
                        .map(|b| b == "main" || b == "develop")
                        .unwrap_or(false);
                    if !is_main {
                        Some(json!({"path": path, "branch": branch}))
                    } else {
                        None
                    }
                })
                .collect();

            Ok(json!({"orphaned_worktrees": orphans}))
        },
    );
}

fn register_build_cleanup_plan(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_BUILD_CLEANUP_PLAN)
            .describe(
                "Build a cleanup confidence score from merged branch and orphaned worktree counts.",
            )
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(
        handlers::TRIAGE_BUILD_CLEANUP_PLAN,
        |input: Value| async move {
            let branches = input
                .get("branches")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let orphans = input
                .get("orphaned_worktrees")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            // confidence high = safe to auto-clean, low = needs manual review
            let confidence: f32 = if orphans > 0 {
                CONFIDENCE_ORPHANED_WORKTREES
            } else if branches > MANY_BRANCHES_THRESHOLD {
                CONFIDENCE_MANY_BRANCHES
            } else {
                CONFIDENCE_CLEAN_STATE
            };

            Ok(HandlerOutput::with_confidence(
                json!({"merged_branch_count": branches, "orphan_count": orphans}),
                confidence,
            ))
        },
    );
}
