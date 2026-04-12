/// Conditional tracing helpers — compile to no-ops without the `tracing` feature.
///
/// Uses `tracing::info!` events rather than spans to avoid `EnteredSpan` (not Send)
/// living across `.await` points in async methods.

/// Emit a tracing event for step execution start.
#[cfg(feature = "tracing")]
macro_rules! trace_step {
    ($name:expr, $confidence:expr) => {
        tracing::info!(name = $name, confidence = $confidence, "step.start");
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_step {
    ($name:expr, $confidence:expr) => {};
}

/// Emit a tracing event for delegation start.
#[cfg(feature = "tracing")]
macro_rules! trace_delegate {
    ($name:expr, $agent:expr) => {
        tracing::info!(name = $name, agent = $agent, "delegate.start");
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_delegate {
    ($name:expr, $agent:expr) => {};
}

/// Emit a tracing event for speculation start.
#[cfg(feature = "tracing")]
macro_rules! trace_speculate {
    ($name:expr, $arm_count:expr) => {
        tracing::info!(name = $name, arms = $arm_count, "speculate.start");
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_speculate {
    ($name:expr, $arm_count:expr) => {};
}

/// Emit a tracing event for join_all start.
#[cfg(feature = "tracing")]
macro_rules! trace_join_all {
    ($name:expr, $arm_count:expr) => {
        tracing::info!(name = $name, arms = $arm_count, "join_all.start");
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_join_all {
    ($name:expr, $arm_count:expr) => {};
}

/// Emit a tracing event for pipe start.
#[cfg(feature = "tracing")]
macro_rules! trace_pipe {
    ($name:expr, $stage_count:expr) => {
        tracing::info!(name = $name, stages = $stage_count, "pipe.start");
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_pipe {
    ($name:expr, $stage_count:expr) => {};
}

/// Emit a tracing event for replay cache hit.
#[cfg(feature = "tracing")]
macro_rules! trace_replay_hit {
    ($name:expr) => {
        tracing::debug!(name = $name, "replay.hit");
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_replay_hit {
    ($name:expr) => {};
}

/// Emit a tracing event for replay cache miss.
#[cfg(feature = "tracing")]
macro_rules! trace_replay_miss {
    ($name:expr) => {
        tracing::debug!(name = $name, "replay.miss");
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_replay_miss {
    ($name:expr) => {};
}

/// Emit a tracing event for hook dispatch.
#[cfg(feature = "tracing")]
macro_rules! trace_hook {
    ($hook:expr, $step:expr) => {
        tracing::debug!(hook = $hook, step = $step, "hook.dispatch");
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_hook {
    ($hook:expr, $step:expr) => {};
}

/// Emit a tracing event for route_on_confidence matching.
#[cfg(feature = "tracing")]
macro_rules! trace_route {
    ($name:expr, $confidence:expr, $label:expr) => {
        tracing::info!(
            name = $name,
            confidence = $confidence,
            matched = $label,
            "route.matched"
        );
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_route {
    ($name:expr, $confidence:expr, $label:expr) => {};
}
