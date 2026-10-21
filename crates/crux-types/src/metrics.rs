//! Shared planner metric and intent types.
//!
//! `RunMetrics` captures resource/timing measurements from a single container
//! run.  `Goal` and `Intent` form the natural-language → pipeline translation
//! pipeline and are consumed by both `crux-planner` and any external crate
//! that drives planning without pulling the full planner dependency.

use serde::{Deserialize, Serialize};

/// Metrics from a single container run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMetrics {
    pub duration_ms: u64,
    pub peak_memory_mb: u64,
    pub exit_code: i32,
    pub success: bool,
}

/// A user-facing goal to be translated into a pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    pub constraints: Vec<String>,
}

/// Parsed intent extracted from a goal — intermediate representation
/// between natural language and a concrete pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub goal: String,
    pub input_source: Option<String>,
    pub output_destination: Option<String>,
    pub constraints: serde_json::Value,
    pub preferences: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_metrics_serde_round_trip() {
        let m = RunMetrics {
            duration_ms: 1500,
            peak_memory_mb: 256,
            exit_code: 0,
            success: true,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: RunMetrics = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn goal_serde_round_trip() {
        let g = Goal {
            description: "fetch the remote data".into(),
            constraints: vec!["max 1s".into(), "no network".into()],
        };
        let json = serde_json::to_string(&g).expect("serialize");
        let back: Goal = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(g, back);
    }

    #[test]
    fn intent_serde_round_trip() {
        let i = Intent {
            goal: "process files".into(),
            input_source: Some("s3://bucket/input".into()),
            output_destination: None,
            constraints: serde_json::json!({"max_steps": 5}),
            preferences: serde_json::json!({"model": "ollama/llama3"}),
        };
        let json = serde_json::to_string(&i).expect("serialize");
        let back: Intent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(i, back);
    }

    #[test]
    fn run_metrics_failed_run() {
        let m = RunMetrics {
            duration_ms: 50,
            peak_memory_mb: 0,
            exit_code: 1,
            success: false,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: RunMetrics = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }
}
