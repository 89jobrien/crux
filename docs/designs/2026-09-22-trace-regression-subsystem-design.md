# Design: Trace Regression Subsystem

## Goal

Provide a deterministic, offline regression system that stores golden Crux traces, evaluates live candidate traces, records structural and metric drift, and fails CI without calling external providers.

## Approved Approach

Build a full regression subsystem with immutable content-addressed artifacts, guarded baseline promotion, policy-driven trace evaluation, append-only report history, a `crux regress` CLI, and an explicit nightly fixture gate.

## Context Map

### Files to Modify

| File | Purpose | Changes Needed |
| --- | --- | --- |
| `Cargo.toml` | Workspace packages and dependencies | Register `crux-regression`, `sha2`, and `fs2` workspace dependencies |
| `Cargo.lock` | Resolved dependency graph | Regenerate after adding the crate and dependencies |
| `crates/crux-improve/src/lib.rs` | Pure trace comparison and evaluation | Add structural policies, invariant selectors, serializable latency semantics, and combined evaluation |
| `crates/crux-improve/README.md` | Improvement API documentation | Document structural regression evaluation and latency representation |
| `crates/crux-cli/Cargo.toml` | CLI dependencies | Add `crux-regression` |
| `crates/crux-cli/src/bin/crux/main.rs` | CLI argument parsing and dispatch | Add nested `regress` commands |
| `crates/crux-cli/src/bin/crux/regress.rs` | Regression CLI adapter | Load policies and traces, open the store, render reports, and set exit codes |
| `crates/crux-cli/tests/regress_cli.rs` | CLI contract tests | Cover ingest, promotion, evaluation, diff, history, and failures |
| `.github/workflows/nightly.yml` | Nightly release gate | Run an explicit credential-free trace evaluation fixture |
| `examples/fixtures/regression/trace.json` | Stable offline baseline fixture | Provide the nightly golden artifact |
| `examples/fixtures/regression/candidate.json` | Stable offline candidate fixture | Exercise evaluation independently from baseline promotion |
| `examples/fixtures/regression/policy.json` | Stable offline policy fixture | Provide strict deterministic thresholds for nightly evaluation |

### Files to Add

| File | Purpose |
| --- | --- |
| `crates/crux-regression/Cargo.toml` | Regression crate manifest |
| `crates/crux-regression/README.md` | Public subsystem usage and storage contract |
| `crates/crux-regression/AGENTS.md` | Crate ownership rules |
| `crates/crux-regression/src/lib.rs` | Public exports |
| `crates/crux-regression/src/artifact.rs` | IDs, versioned envelopes, canonical digesting |
| `crates/crux-regression/src/error.rs` | Regression storage and orchestration errors |
| `crates/crux-regression/src/harness.rs` | Baseline lifecycle and evaluation orchestration |
| `crates/crux-regression/src/report.rs` | Persisted report envelope |
| `crates/crux-regression/src/store/mod.rs` | `RegressionStore` port and conformance helpers |
| `crates/crux-regression/src/store/file.rs` | Locked atomic filesystem adapter |
| `crates/crux-regression/src/store/memory.rs` | In-memory adapter |
| `crates/crux-regression/tests/store_conformance.rs` | Shared adapter contract tests |

### Dependencies

```text
crux-types <- crux-schema <- crux-improve <- crux-regression <- crux-cli
```

- `crux-improve` remains the owner of pure comparison, diff, metric, and policy types.
- `crux-regression` owns persistence, baseline lifecycle, orchestration, and report history.
- `crux-cli` is only the composition and presentation boundary.
- `crux-regression` must not depend on `crux-runtime`, `crux-script`, `crux-agentic`, or `crux-cli`.

### Existing Coverage and Reference Patterns

| File | Pattern to Follow |
| --- | --- |
| `crates/crux-improve/src/lib.rs` | Existing `EvalHarness`, thresholds, metrics, and reports |
| `crates/crux-runtime/src/replay.rs` | Replay matching and property-test conventions |
| `crates/crux-task/tests/backend_conformance.rs` | Reusable adapter conformance suite |
| `crates/crux-cli/tests/moa_review_cli_regressions.rs` | Isolated `HOME` and subprocess CLI assertions |
| `crates/crux-cli/src/bin/crux/replay_debug.rs` | Raw `Crux<Value>` trace loading |
| `crates/crux-planner/tests/llm_planner_snapshots.rs` | Focused `insta` snapshot conventions |
| `.github/workflows/nightly.yml` | Existing credential-free regression job |

## Crate Ownership

- **Pure evaluation owner**: `crux-improve` -- trace projections, structural diffing, invariant evaluation, metric thresholds, and combined pass/fail decisions.
- **Lifecycle owner**: `crux-regression` -- immutable artifacts, baseline references, CAS promotion, evaluation orchestration, and history.
- **Application adapter**: `crux-cli` -- filesystem composition, policy loading, output rendering, and process exit status.

## Public API

### `crux-improve`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "ratio", rename_all = "snake_case")]
pub enum LatencyRatio {
    Finite(f32),
    Unbounded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatencyComparison {
    pub baseline_ms: u64,
    pub candidate_ms: u64,
    pub ratio: LatencyRatio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalReport {
    pub passed: bool,
    pub comparison: Comparison,
    pub latency_ratio: f32,
    pub latency: LatencyComparison,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDiffMode {
    Strict,
    Compatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateTracePolicy {
    LiveOnly,
    AllowReplayed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralPolicy {
    pub mode: TraceDiffMode,
    pub confidence_tolerance: f32,
    pub compare_outputs: bool,
    pub candidate_trace: CandidateTracePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TracePath(pub Vec<usize>);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum StepIdentity {
    StableId(String),
    Name(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOccurrence {
    ExactlyOne,
    AtLeastOne,
    All,
    Nth(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepSelector {
    pub trace_path: TracePath,
    pub identity: StepIdentity,
    pub occurrence: StepOccurrence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepInvariant {
    pub id: String,
    pub selector: StepSelector,
    pub expected_status: StepStatus,
    pub minimum_confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionPolicy {
    pub metrics: EvalThresholds,
    pub structure: StructuralPolicy,
    pub expected_output: Option<Value>,
    pub invariants: Vec<StepInvariant>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepDiff {
    pub trace_path: TracePath,
    pub baseline_index: Option<usize>,
    pub candidate_index: Option<usize>,
    pub identity: StepIdentity,
    pub changes: Vec<StepChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "field", content = "values", rename_all = "snake_case")]
pub enum StepChange {
    Added,
    Removed,
    Kind { baseline: StepKind, candidate: StepKind },
    Status { baseline: StepStatus, candidate: StepStatus },
    Confidence { baseline: f32, candidate: f32 },
    InputHash { baseline: u64, candidate: u64 },
    ContentHash { baseline: Option<u64>, candidate: Option<u64> },
    Output { baseline: Option<Value>, candidate: Option<Value> },
    Error { baseline: Option<String>, candidate: Option<String> },
    Attempt { baseline: u32, candidate: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceDiff {
    pub mode: TraceDiffMode,
    pub matches: bool,
    pub step_diffs: Vec<StepDiff>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantViolation {
    pub invariant_id: String,
    pub selector: StepSelector,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionEvaluation {
    pub passed: bool,
    pub metrics: EvalReport,
    pub structure: TraceDiff,
    pub invariant_violations: Vec<InvariantViolation>,
    pub failures: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EvalConfigError {
    #[error("{field} must be finite, got {value}")]
    NonFiniteThreshold { field: &'static str, value: f32 },
    #[error("confidence tolerance must be non-negative, got {0}")]
    NegativeConfidenceTolerance(f32),
    #[error("duplicate invariant id '{0}'")]
    DuplicateInvariantId(String),
    #[error("invariant '{id}' has invalid configuration: {message}")]
    InvalidInvariant { id: String, message: String },
    #[error("candidate trace contains replayed step '{step}' at {path:?}")]
    ReplayedCandidateStep { path: TracePath, step: String },
}

impl EvalHarness {
    pub fn new(thresholds: EvalThresholds) -> Self;
    pub fn try_new(thresholds: EvalThresholds) -> Result<Self, EvalConfigError>;
    pub fn with_golden_answer(self, answer: Value) -> Self;
    pub fn evaluate<T: Serialize>(
        &self,
        baseline: &Crux<T>,
        candidate: &Crux<T>,
    ) -> EvalReport;
}

pub fn diff_traces<T: Serialize>(
    baseline: &Crux<T>,
    candidate: &Crux<T>,
    policy: &StructuralPolicy,
) -> Result<TraceDiff, EvalConfigError>;

pub fn evaluate_regression<T: Serialize>(
    baseline: &Crux<T>,
    candidate: &Crux<T>,
    policy: &RegressionPolicy,
) -> Result<RegressionEvaluation, EvalConfigError>;
```

`EvalReport::latency_ratio` and infallible `EvalHarness::new` remain as compatibility shims.
`EvalReport::latency` carries the lossless finite/unbounded representation, while
`EvalHarness::try_new` validates externally supplied thresholds.

### `crux-regression`

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RegressionCaseId(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraceDigest([u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RegressionRunId(String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TraceEnvelope {
    format_version: u16,
    digest_algorithm: String,
    trace: Crux<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineRef {
    pub case: RegressionCaseId,
    pub trace: TraceDigest,
    pub labels: BTreeMap<String, String>,
    pub promoted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionReport {
    pub id: RegressionRunId,
    pub case: RegressionCaseId,
    pub baseline: TraceDigest,
    pub candidate: TraceDigest,
    pub policy: RegressionPolicy,
    pub evaluation: RegressionEvaluation,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegressionError {
    #[error("invalid regression case id '{0}'")]
    InvalidCaseId(String),
    #[error("invalid trace digest '{0}'")]
    InvalidDigest(String),
    #[error("unsupported trace envelope version {0}")]
    UnsupportedFormatVersion(u16),
    #[error("unsupported digest algorithm '{0}'")]
    UnsupportedDigestAlgorithm(String),
    #[error("trace artifact {0} was not found")]
    ArtifactNotFound(TraceDigest),
    #[error("baseline for case {0} was not found")]
    BaselineNotFound(RegressionCaseId),
    #[error("trace digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: TraceDigest, actual: TraceDigest },
    #[error("baseline conflict for {case}: expected {expected:?}, got {actual:?}")]
    BaselineConflict {
        case: RegressionCaseId,
        expected: Option<TraceDigest>,
        actual: Option<TraceDigest>,
    },
    #[error("corrupt regression artifact at {path}: {message}")]
    CorruptArtifact { path: PathBuf, message: String },
    #[error("regression I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("regression serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Evaluation(#[from] EvalConfigError),
}

pub trait RegressionStore: Send + Sync {
    fn put_trace(&self, trace: &Crux<Value>) -> Result<TraceDigest, RegressionError>;
    fn trace(&self, digest: &TraceDigest) -> Result<Crux<Value>, RegressionError>;
    fn compare_and_set_baseline(
        &self,
        case: &RegressionCaseId,
        expected: Option<&TraceDigest>,
        next: &BaselineRef,
    ) -> Result<(), RegressionError>;
    fn baseline(&self, case: &RegressionCaseId) -> Result<BaselineRef, RegressionError>;
    fn append_report(&self, report: &RegressionReport) -> Result<(), RegressionError>;
    fn reports(
        &self,
        case: &RegressionCaseId,
    ) -> Result<Vec<RegressionReport>, RegressionError>;
}

#[derive(Debug)]
pub struct RegressionHarness<S> {
    store: S,
}

impl<S: RegressionStore> RegressionHarness<S> {
    pub fn new(store: S) -> Self;
    pub fn ingest(&self, trace: &Crux<Value>) -> Result<TraceDigest, RegressionError>;
    pub fn promote(
        &self,
        case: RegressionCaseId,
        digest: TraceDigest,
        expected: Option<TraceDigest>,
        labels: BTreeMap<String, String>,
    ) -> Result<BaselineRef, RegressionError>;
    pub fn evaluate(
        &self,
        case: &RegressionCaseId,
        candidate: &Crux<Value>,
        policy: &RegressionPolicy,
    ) -> Result<RegressionReport, RegressionError>;
    pub fn diff(
        &self,
        case: &RegressionCaseId,
        candidate: &Crux<Value>,
        policy: &StructuralPolicy,
    ) -> Result<TraceDiff, RegressionError>;
    pub fn history(
        &self,
        case: &RegressionCaseId,
    ) -> Result<Vec<RegressionReport>, RegressionError>;
}

#[derive(Debug, Default)]
pub struct InMemoryRegressionStore {
    state: Arc<Mutex<InMemoryStoreState>>,
}

#[derive(Debug, Default)]
struct InMemoryStoreState {
    objects: BTreeMap<TraceDigest, Crux<Value>>,
    baselines: BTreeMap<RegressionCaseId, BaselineRef>,
    reports: BTreeMap<RegressionCaseId, Vec<RegressionReport>>,
}

#[derive(Debug)]
pub struct FileRegressionStore {
    root: PathBuf,
}

impl FileRegressionStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RegressionError>;
}

impl RegressionCaseId {
    pub fn new(value: impl Into<String>) -> Result<Self, RegressionError>;
    pub fn as_str(&self) -> &str;
}

impl TraceDigest {
    pub fn as_bytes(&self) -> &[u8; 32];
}

impl RegressionRunId {
    pub fn new() -> Self;
    pub fn as_str(&self) -> &str;
}
```

`RegressionCaseId` and `RegressionRunId` implement `Display`, `FromStr`, and `AsRef<str>`. `TraceDigest` implements `Display`, `FromStr`, and `AsRef<[u8]>`. All three use serde string representations. `RegressionRunId` also implements `Default`. `RegressionError` implements `std::error::Error` through `thiserror`.

## Diff Semantics

### Artifact Identity

- The artifact digest is not a semantic trace hash.
- Digest input is `b"crux-trace-artifact:v1\0"` followed by recursively key-sorted JSON for the complete raw `Crux<Value>` trace.
- IDs, timestamps, durations, attempts, metadata, findings, events, and replay origin participate in artifact identity.
- The envelope records `format_version = 1` and `digest_algorithm = "sha256"`.
- Readers reject unknown versions, unknown algorithms, malformed digests, and digest mismatches.

### Strict Mode

- Child traces are compared recursively by serialized child index.
- Step count and order must match within every trace path.
- Step identity, kind, status, input hash, content hash, attempt, error, and configured output must match.
- Confidence may differ only within `confidence_tolerance`.
- Timestamps, durations, and replay origin are not structural fields; duration is evaluated separately and origin is enforced by `CandidateTracePolicy`.

### Compatible Mode

- Every baseline child path and baseline step must remain present in candidate order.
- Matching uses trace path plus stable ID when present, otherwise name, plus occurrence ordinal.
- Added child traces and steps are allowed only when every added step has `StepStatus::Ok`.
- Removed baseline steps, reordered baseline steps, changed kinds or statuses, new errors/rejections/skips, and invariant violations fail.
- Content hashes and outputs may drift unless an invariant or expected output constrains them.

### Invariants

- A selector identifies one serialized trace path and a stable ID or name.
- `ExactlyOne`, `AtLeastOne`, `All`, and `Nth` make repeated loop/retry semantics explicit.
- All invariant violations are collected in deterministic policy order.
- Duplicate invariant IDs and non-finite confidence thresholds are rejected during policy validation.

### Latency and Provenance

- `LiveOnly` rejects a candidate containing any replayed step.
- `AllowReplayed` permits replayed steps but is opt-in because replay durations are zero.
- Zero baseline and zero candidate duration produce `LatencyRatio::Finite(1.0)`.
- Zero baseline and nonzero candidate duration produce `LatencyRatio::Unbounded`.
- An unbounded ratio fails any finite maximum latency policy and remains valid JSON.

## Storage Semantics

```text
<store>/
  objects/<sha256-hex>.json
  baselines/<case-id>.json
  reports/<case-id>/<run-id>.json
  locks/<case-id>.lock
```

- Object and report files are immutable.
- Baseline refs are updated under a cross-process advisory lock using temp-file plus atomic rename.
- Initial promotion expects no current digest.
- Replacement promotion must provide the current digest; stale promotion returns a conflict containing expected and actual digests.
- `RegressionCaseId` accepts only non-empty ASCII alphanumeric strings plus `.`, `_`, and `-`.
- Every resolved path remains below the configured store root.
- Reports sort newest first by run ID, whose string form is a ULID.
- Evaluation stores the candidate object and append-only report before returning pass/fail.

## CLI

```text
crux regress ingest TRACE [--store PATH]
crux regress promote CASE DIGEST [--expected-current DIGEST] [--label KEY=VALUE] [--store PATH]
crux regress evaluate CASE CANDIDATE --policy POLICY [--json] [--store PATH]
crux regress diff CASE CANDIDATE --policy POLICY [--json] [--store PATH]
crux regress history CASE [--json] [--store PATH]
```

- Default store: `$HOME/.crux/regressions`.
- `ingest` prints only the digest on stdout.
- Human output uses stable labels, not full serialized reports.
- JSON output serializes the corresponding public report or diff type.
- Invalid input, missing artifacts, corruption, and CAS conflicts exit nonzero.
- `evaluate` exits zero only when `RegressionReport::evaluation.passed` is true.
- No command promotes or rewrites a baseline implicitly.

## Nightly CI Gate

The existing nightly `regression` job gains a credential-free step after `cargo nextest run`:

1. Build `crux-cli`.
2. Create a store under `$RUNNER_TEMP`.
3. Ingest `examples/fixtures/regression/trace.json` as the baseline.
4. Promote it as the initial `nightly-showcase` baseline.
5. Evaluate `examples/fixtures/regression/candidate.json` with `examples/fixtures/regression/policy.json`.
6. Write the JSON report under `$RUNNER_TEMP` and upload it with `actions/upload-artifact` using `if: always()`.

The fixture uses no credentials, network, wall-clock assertions, or automatic baseline updates. The existing `nightly-release` job remains gated on the `regression` job.

## Testing

### Unit

- Versioned digest golden vector, canonical map ordering, parse/display, and mutation sensitivity.
- Strict/compatible table covering identity, ordering, insertion, removal, retries, child paths, outputs, confidence, and candidate origin.
- Selector occurrence and invariant aggregation behavior.
- Finite, threshold-boundary, zero, and unbounded latency behavior.
- Combined report ordering and `passed == failures.is_empty()`.

### Property

- Digest survives serde roundtrip.
- JSON map-key permutation leaves digest unchanged.
- Strict diff is reflexive.
- Mutating a strict semantic field produces a difference.
- Stored traces roundtrip and verify against their digest.

### Store Conformance

- Run one reusable contract against in-memory and filesystem adapters.
- Cover missing objects/refs, idempotent puts, immutable objects, initial/successful/stale CAS, exactly one concurrent CAS winner, corruption, failed-CAS non-mutation, and report ordering.
- Filesystem-only coverage verifies reopen persistence and that readers never observe partial writes.

### Integration

- Identical baseline/candidate passes.
- Strict mutation fails with a structured diff.
- Approved compatible insertion passes compatible mode and fails strict mode.
- Structural, metric, and invariant failures compose in one report.
- Reopened filesystem store evaluates against its persisted baseline.

### CLI

- One success and principal failure per subcommand.
- Parse JSON output structurally; only assert stable labels in human output.
- Cover stale promotion, corruption, digest mismatch, strict/compatible behavior, and nonzero evaluation exit.
- Execute the exact nightly fixture command in one integration test.

### Snapshots

- At most one normalized human report and one normalized JSON report snapshot.
- Scrub IDs, timestamps, durations, paths, and other volatile values.

## Out of Scope

- Selective replay that invalidates only model steps.
- Direct model/provider execution or checkpoint substitution.
- Token and cost drift until usage is a normalized trace field.
- Remote, SQLite, redb, or object-store adapters.
- Multi-case suite manifests and batch execution.
- Automatic baseline promotion or snapshot acceptance.
- Dashboards and approval user interfaces.
- Inferring delegation relationships not represented in serialized child traces.

## Risk

- **Breaking API change**: no -- legacy `EvalHarness::new` and `EvalReport::latency_ratio` remain available while validated and lossless alternatives are added.
- **Serialization format change**: yes -- regression artifacts use a versioned envelope; raw legacy traces remain accepted as CLI inputs but are stored as V1 envelopes.
- **New external dependencies**: yes -- `sha2` for stable content addressing and `fs2` for cross-process baseline locking.
- **Feature flag required**: no -- the subsystem is credential-free and has no provider dependency.
- **Dirty-tree dependency**: implementation assumes the active `StepOrigin` work is retained and passing; it must not overwrite or revert those changes.
- **Parallel duration limitation**: total step duration remains the existing `TraceMetrics` definition and may overcount parallel work; this is documented rather than silently presented as wall-clock latency.
