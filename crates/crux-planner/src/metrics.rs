use serde::{Deserialize, Serialize};

/// Metrics from a single container run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetrics {
    pub duration_ms: u64,
    pub peak_memory_mb: u64,
    pub exit_code: i32,
    pub success: bool,
}
