# crux-regression

Deterministic lifecycle support for Crux golden traces. The crate owns content-addressed trace
artifacts, guarded baseline promotion, append-only evaluation history, and orchestration around the
pure policies in `crux-improve`.

Artifacts use a versioned SHA-256 digest over canonical raw trace JSON. Runtime replay and model
execution remain outside this crate.

Trace artifacts and output-comparing reports can contain raw model or tool data. Keep the store in
a trusted private directory, redact secrets before ingestion, and enable output comparison only
when persisted values are appropriate for the target environment.

`RegressionHarness` provides the lifecycle: ingest a trace, explicitly promote its digest with a
compare-and-set guard, evaluate candidates with `crux-improve::RegressionPolicy`, and query
append-only report history. `InMemoryRegressionStore` supports tests while `FileRegressionStore`
persists the same contract atomically on disk.

## Development

```console
cargo nextest run -p crux-regression
cargo clippy -p crux-regression --all-targets -- -D warnings
```
