# crux-improve

Serializable vocabulary and deterministic calculations for comparing agent traces and proposing
strategy changes. It is a protocol/data crate, not an autonomous improvement runtime.

## Architecture role

The crate builds on `crux-types` and re-exports the trace types needed by improvement consumers.
Runtime execution belongs in `crux-runtime`; model-backed analysis belongs in `crux-agentic`.

## Usage

```rust
use crux_improve::{TraceMetrics, Verdict, replay_compare};

let comparison = replay_compare(&old_trace, &new_trace);
if comparison.verdict == Verdict::Improved {
    println!("score delta: {}", comparison.delta);
}
# let _ = TraceMetrics::extract(&new_trace);
```

## Key API

- `TraceMetrics::extract`: step count, success/error rates, confidence, duration, delegation depth,
  speculation hit rate, and a weighted score.
- `replay_compare`: compares two traces; deltas beyond +/-0.05 are improved or regressed.
- `Strategy` and `StrategyDiff`: versioned tool preferences, confidence thresholds, and prompt
  patches; `Strategy::apply` increments the version.
- `Improvement` and `ImprovementKind`: evidence-backed proposal records.
- `Comparison` and `Verdict`: old/new metrics and classified delta.
- `StrategyPolicy`, `DefaultStrategyPolicy`, `StrategyViolation`: validation and approval port.
- Re-exports: `Crux<T>`, `CruxId`, `Step`, `StepKind`, and `StepStatus`.

## Features and implementation status

There are no feature flags or generated files. Scoring currently weights success rate at 60% and
average confidence at 40%. `DefaultStrategyPolicy` can cap tool preferences and requires approval
for prompt patches. Persisted-run proposal/approval orchestration is not implemented in this crate.

## Development and testing

```console
cargo nextest run -p crux-improve
cargo clippy -p crux-improve --all-targets -- -D warnings
```

Unit tests cover empty and populated metrics, strategy application and serde, comparison verdicts,
and default policy behavior.
