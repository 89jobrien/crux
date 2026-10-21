# Design: Governed HTTP Execution E2E

## Goal

Prove that Crux runtime execution emits a durable, cryptographically authorized Execution Envelope
V1 lifecycle while invoking a deterministic local HTTP endpoint and suppressing unauthorized side
effects.

## Approved Approach

Use a **Governed Runtime Executor** in `crux-runtime`, keep external verification, admission,
persistence, nonce handling, clocks, and HTTP behavior behind ports, and exercise fixed-proof and
strict Ed25519 paths through nextest plus a runnable loopback demo. HTTP is the first deterministic
test action adapter; Minibox remains the first planned production executor from the V1 envelope
design.

## Constraints

- Loopback networking only; no public internet, credentials, or paid providers.
- Runtime orchestration remains independent of HTTP, filesystems, and Ed25519 libraries.
- Every valid envelope prefix is persisted before the next externally visible transition.
- Invalid authorization or denied admission must prevent the HTTP action from running.
- HTTP response bytes are stored as external evidence; the envelope stores digest, size, media type,
  and locator only.
- Tests use deterministic keys and clocks where golden behavior matters.
- The existing Execution Envelope V1 implementation is a prerequisite and lands separately.

## Context Map

### Prerequisite Files

The uncommitted V1 implementation currently lives in
`/Users/joe/dev/crux/.worktrees/execution-envelope-v1` and must be reconciled onto current
`develop` without replacing newer workspace or schema-split changes.

| File | Purpose | Required Before Runtime Integration |
| --- | --- | --- |
| `Cargo.toml` | Workspace dependencies | Merge `base64`, `hex`, `jcs-admit`, and `sha2`; preserve current dependencies |
| `crates/crux-types/Cargo.toml` | Wire dependencies and MSRV | Add V1 dependencies and independently verify Rust 1.85 |
| `crates/crux-types/src/execution/mod.rs` | Envelope V1 contract | Land implementation, ceilings, result validation, and verifier accessors |
| `crates/crux-types/src/lib.rs` | Public modules | Export `execution` without regressing `crux-schema` split |
| `crates/crux-types/tests/execution_envelope.rs` | Contract suite | Land lifecycle, tamper, canonicalization, and golden-vector coverage |
| `crates/crux-types/tests/fixtures/*.json` | Golden vectors | Preserve canonical record and authorization bytes |

Required V1 API corrections:

- add `ExecutionEnvelopeV1::intent()`;
- expose authorization algorithm, issuance, expiry, and nonce through borrowing accessors;
- expose decoded proof signature bytes without exposing a crypto-library type;
- make `ExecutionResultV1::new` validating and fallible;
- enforce documented parent, collection, evidence, string, JSON, record, and envelope ceilings;
- define `NetworkEndpointV1::port == None` as the scheme's known default port;
- define `ExecutionStartedV1::started_at` as the durable release-authorization instant: external
  work may begin only after that record is persisted, and the timestamp does not claim to be the
  first network-byte or process-instruction instant.

```rust
impl AuthorizationStatementV1 {
    pub const fn algorithm(&self) -> SignatureAlgorithmV1;
    pub const fn issued_at(&self) -> &DateTime<Utc>;
    pub const fn expires_at(&self) -> &DateTime<Utc>;
    pub const fn nonce(&self) -> &AuthorizationNonce;
}

impl AuthorizationProofV1 {
    pub fn signature_bytes(&self) -> Result<[u8; 64], ExecutionEnvelopeError>;
}

impl ExecutionEnvelopeV1 {
    pub fn intent(&self) -> &ExecutionIntentV1;
}

impl ExecutionResultV1 {
    pub fn new(
        status: ExecutionStatusV1,
        completed_at: DateTime<Utc>,
        exit_code: Option<i32>,
        observation: Option<serde_json::Value>,
        error: Option<ExecutionErrorV1>,
        replay: ReplayMetadataV1,
    ) -> Result<Self, ExecutionEnvelopeError>;
}
```

### Files To Modify

| File | Purpose | Change |
| --- | --- | --- |
| `Cargo.toml` | Shared dependencies | Add `ed25519-dalek` and `fs2` as workspace dependencies |
| `Cargo.lock` | Dependency resolution | Record wire and crypto dependencies |
| `crates/crux-runtime/src/governed_execution.rs` | Runtime orchestration | Add ports, staging values, errors, in-memory store, and executor |
| `crates/crux-runtime/src/lib.rs` | Public modules | Export governed execution |
| `crates/crux-runtime/src/types/mod.rs` | Wire re-export | Re-export `crux_types::execution` |
| `crates/crux-runtime/tests/governed_execution.rs` | Port/orchestration tests | Cover ordering, persistence, denial, failures, and CAS |
| `crates/crux-agentic/Cargo.toml` | Adapter dependencies/features | Add crypto dependency and `test-utils` feature |
| `crates/crux-agentic/src/adapters/mod.rs` | Adapter exports | Export authorization, admission, store, and HTTP adapters |
| `crates/crux-agentic/src/lib.rs` | Public modules/features | Export adapters and feature-gated `test_utils` |
| `crates/crux-agentic/src/adapters/execution_authorization.rs` | Crypto adapter | Strict Ed25519 verification, audience, expiry, and trusted-key checks |
| `crates/crux-agentic/src/adapters/execution_nonce.rs` | Replay adapter | Process-local nonce reservation, commit, and release |
| `crates/crux-agentic/src/adapters/execution_admission.rs` | Admission adapter | Enforce requested, signed, executor-maximum capability containment |
| `crates/crux-agentic/src/adapters/execution_store.rs` | Persistence adapter | Atomic JSON snapshot store with head-digest CAS |
| `crates/crux-agentic/src/adapters/http_execution.rs` | Action adapter | Map signed capabilities and HTTP action input to staged execution |
| `crates/crux-agentic/src/http.rs` | Shared transport | Extract bounded transport while preserving `http::request` behavior |
| `crates/crux-agentic/src/test_utils/mod.rs` | Feature-gated test support | Export local server and deterministic fixture builders |
| `crates/crux-agentic/src/test_utils/local_http.rs` | Local HTTP fixture | Serve deterministic endpoints and request counters |
| `crates/crux-agentic/tests/http_execution.rs` | Adapter tests | Verify capability-derived network policy and evidence limits |
| `crates/crux-agentic/examples/governed_http.rs` | Runnable demo | Exercise success, invalid signature, and admission denial |
| `crates/crux/Cargo.toml` | Facade dev dependencies | Enable agentic test utilities and signing fixtures |
| `crates/crux/tests/governed_http.rs` | Full E2E | Run governed executor through real local HTTP adapter and persistence |

### Dependency Edges

```text
crux-types
    ^
crux-runtime
    ^
crux-agentic

crux -> crux-runtime
crux --dev-> crux-agentic(test-utils)
```

`crux-runtime` must not depend on `crux-agentic`, `reqwest`, or `ed25519-dalek`. The facade test is
the composition root for runtime and adapter implementations.

### Existing Coverage

| File | Existing Coverage | Gap Filled Here |
| --- | --- | --- |
| `crux-types/tests/execution_envelope.rs` in V1 worktree | Wire lifecycle, canonical vectors, capability containment, tamper checks | Real signature and runtime emission remain absent |
| `crates/crux-runtime/src/approval.rs` | Async port pattern | No execution authorization verifier |
| `crates/crux-runtime/src/registry/backend.rs` | Async persistence/CAS pattern | No envelope snapshot store |
| `crates/crux-agentic/tests/http.rs` | Ephemeral local HTTP server and handler bounds | Policy comes from request args, not signed capabilities |
| `crates/crux/tests/substrate_integration.rs` | Facade-level runtime composition | No governed execution lifecycle |

### Reference Patterns

- `crates/crux-runtime/src/approval.rs`: RPITIT async ports.
- `crates/crux-runtime/src/registry/backend.rs`: persistence and CAS port shape.
- `crates/crux-runtime/src/audit.rs`: in-memory append-only adapter.
- `crates/crux-agentic/src/adapters/terminal_approval.rs`: concrete runtime-port adapter.
- `crates/crux-agentic/src/http.rs`: bounded HTTP transport and local-network policy.
- `crates/crux-agentic/tests/http.rs`: loopback fixture pattern.
- `crates/crux/tests/substrate_integration.rs`: public facade E2E composition.

### Risk

- The V1 feature worktree is behind current `develop`; wholesale file copies would regress newer APIs.
- Signature verification must operate on `AuthorizationStatementV1::signing_bytes()`, not its
  payload digest.
- Store failures after a transition must not leave the caller-visible envelope ahead of durable
  state.
- HTTP authority must derive only from admitted capabilities; caller-supplied policy is forbidden.
- A process-local nonce set demonstrates replay rejection but is not durable production protection.
- E2E tests cross three production crates; crate boundaries must remain one-directional.

## Crate Ownership

- **`crux-types`**: transport-neutral V1 wire values and pure validation; prerequisite only.
- **`crux-runtime`**: six execution ports, staging values, lifecycle orchestration, and runtime
  errors.
- **`crux-agentic`**: Ed25519, capability admission, atomic JSON persistence, HTTP evidence, and
  local-server adapters.
- **`crux` facade**: integration tests only; no domain logic.

## Public API

### Runtime Port Error

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ExecutionPortError {
    code: String,
    message: String,
}

impl ExecutionPortError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self;
    pub fn code(&self) -> &str;
    pub fn message(&self) -> &str;
}
```

### Authorization And Admission Ports

```rust
pub trait AuthorizationVerifier: Send + Sync {
    fn verify(
        &self,
        authorization: &AuthorizationRecordV1,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), ExecutionPortError>> + Send;
}

pub trait AdmissionPolicy: Send + Sync {
    fn admit(
        &self,
        intent: &ExecutionIntentV1,
        authorization: &AuthorizationRecordV1,
    ) -> impl Future<Output = Result<AdmissionRecordV1, ExecutionPortError>> + Send;
}
```

The verifier owns trusted keys, audience, and expiry checks. Nonce replay is coordinated separately
with durable-baseline persistence.

### Nonce And Clock Ports

```rust
pub trait NonceStore: Send + Sync {
    type Reservation: Send;

    fn reserve(
        &self,
        authorization: &AuthorizationRecordV1,
    ) -> impl Future<Output = Result<Self::Reservation, ExecutionPortError>> + Send;

    fn commit(
        &self,
        reservation: Self::Reservation,
    ) -> impl Future<Output = Result<(), ExecutionPortError>> + Send;

    fn release(
        &self,
        reservation: Self::Reservation,
    ) -> impl Future<Output = Result<(), ExecutionPortError>> + Send;
}

pub trait ExecutionClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemExecutionClock;
```

Nonce reservation occurs only for executable authorization decisions. A baseline conflict or
pre-rename I/O failure releases the reservation. Baseline `CommitIndeterminate` retains the
reservation and requires store reconciliation. Successful baseline persistence commits it; a nonce
commit failure remains fail-closed and no action starts.

### Envelope Store Port

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedEnvelopeHead {
    Absent,
    Digest(ContentDigestV1),
}

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeStoreError {
    #[error("envelope compare-and-swap conflict")]
    Conflict,
    #[error("envelope store I/O failed: {message}")]
    Io { message: String },
    #[error("envelope commit may be visible but is not crash-durable: {message}")]
    CommitIndeterminate { message: String },
}

pub trait EnvelopeStore: Send + Sync {
    fn load(
        &self,
        execution_id: &ExecutionId,
    ) -> impl Future<Output = Result<Option<ExecutionEnvelopeV1>, EnvelopeStoreError>> + Send;

    fn compare_and_swap(
        &self,
        envelope: &ExecutionEnvelopeV1,
        expected: ExpectedEnvelopeHead,
    ) -> impl Future<Output = Result<(), EnvelopeStoreError>> + Send;
}

pub struct InMemoryEnvelopeStore {
    snapshots: Mutex<HashMap<ExecutionId, Vec<ExecutionEnvelopeV1>>>,
}

impl InMemoryEnvelopeStore {
    pub fn new() -> Self;
    pub fn snapshots(&self, execution_id: &ExecutionId) -> Vec<ExecutionEnvelopeV1>;
}
```

`ExpectedEnvelopeHead::Digest` is the digest of the final record in the last durable prefix. A
conflict or pre-rename I/O failure leaves durable state unchanged. A post-rename parent-directory
sync failure returns `CommitIndeterminate`; callers must reconcile by loading the store before any
retry. When this occurs before `ActionExecutor::execute`, no action starts.

### Staged Action Port

```rust
#[derive(Debug)]
pub struct PreparedExecution<T> {
    environment: ExecutionEnvironmentV1,
    state: T,
}

impl<T> PreparedExecution<T> {
    pub fn new(environment: ExecutionEnvironmentV1, state: T) -> Self;
    pub fn into_parts(self) -> (ExecutionEnvironmentV1, T);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCompletion {
    outputs: Vec<ExecutionOutputV1>,
    artifacts: Vec<ArtifactRefV1>,
    status: ExecutionStatusV1,
    exit_code: Option<i32>,
    observation: Option<serde_json::Value>,
    error: Option<ExecutionErrorV1>,
    replay: ReplayMetadataV1,
}

impl ActionCompletion {
    pub fn new(
        outputs: Vec<ExecutionOutputV1>,
        artifacts: Vec<ArtifactRefV1>,
        status: ExecutionStatusV1,
        exit_code: Option<i32>,
        observation: Option<serde_json::Value>,
        error: Option<ExecutionErrorV1>,
        replay: ReplayMetadataV1,
    ) -> Result<Self, ExecutionEnvelopeError>;
}

pub trait ActionExecutor: Send + Sync {
    type Prepared: Send;

    fn prepare(
        &self,
        action: &ActionSpecV1,
        capabilities: &CapabilitySetV1,
        output: &OutputContractV1,
    ) -> impl Future<Output = Result<PreparedExecution<Self::Prepared>, ExecutionPortError>> + Send;

    fn execute(
        &self,
        prepared: Self::Prepared,
    ) -> impl Future<Output = Result<ActionCompletion, ExecutionPortError>> + Send;
}
```

The runtime persists `Environment`, appends and persists `Started` using its clock, and only then
calls `ActionExecutor::execute`; the adapter cannot release the HTTP side effect earlier.

### Governed Executor

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionStage {
    Prepare,
    Execute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernedExecutionOutcome {
    AuthorizationDenied,
    ReviewRequired { review_id: String },
    RetryLater { not_before: DateTime<Utc> },
    AdmissionDenied,
    Completed(ExecutionStatusV1),
}

#[derive(Debug, thiserror::Error)]
pub enum GovernedExecutionError {
    #[error("invalid execution envelope: {0}")]
    Envelope(#[from] ExecutionEnvelopeError),
    #[error("authorization verification failed: {0}")]
    Authorization(#[source] ExecutionPortError),
    #[error("admission failed: {0}")]
    Admission(#[source] ExecutionPortError),
    #[error("nonce reservation failed: {0}")]
    Nonce(#[source] ExecutionPortError),
    #[error("envelope persistence failed: {0}")]
    Store(#[from] EnvelopeStoreError),
    #[error("action {stage:?} failed: {source}")]
    Action {
        stage: ActionStage,
        #[source]
        source: ExecutionPortError,
    },
}

pub struct GovernedExecutor<V, P, S, N, A, C> {
    verifier: V,
    admission: P,
    store: S,
    nonces: N,
    action: A,
    clock: C,
}

impl<V, P, S, N, A, C> GovernedExecutor<V, P, S, N, A, C>
where
    V: AuthorizationVerifier,
    P: AdmissionPolicy,
    S: EnvelopeStore,
    N: NonceStore,
    A: ActionExecutor,
    C: ExecutionClock,
{
    pub fn new(verifier: V, admission: P, store: S, nonces: N, action: A, clock: C) -> Self;

    pub async fn execute(
        &self,
        envelope: &mut ExecutionEnvelopeV1,
    ) -> Result<GovernedExecutionOutcome, GovernedExecutionError>;
}
```

The executor requires an envelope containing exactly `Intent -> Authorization`. Signed deny,
review, and retry-later decisions are verified, persisted, and returned without admission or action.
The executor does not create, sign, or mutate authorization statements.

### Agentic Adapters

```rust
#[derive(Debug, thiserror::Error)]
pub enum Ed25519VerifierConfigError {
    #[error("authorization audience cannot be empty")]
    EmptyAudience,
    #[error("duplicate trusted key id '{key_id}'")]
    DuplicateKeyId { key_id: String },
    #[error("invalid Ed25519 public key for '{key_id}'")]
    InvalidPublicKey { key_id: String },
}

pub struct Ed25519AuthorizationVerifier {
    audience: String,
    keys: BTreeMap<String, [u8; 32]>,
}

impl Ed25519AuthorizationVerifier {
    pub fn new(
        audience: impl Into<String>,
        keys: impl IntoIterator<Item = (String, [u8; 32])>,
    ) -> Result<Self, Ed25519VerifierConfigError>;
}

pub struct ProcessLocalNonceStore {
    reserved: Mutex<HashSet<AuthorizationNonce>>,
    committed: Mutex<HashSet<AuthorizationNonce>>,
}

impl ProcessLocalNonceStore {
    pub fn new() -> Self;
}

pub struct CapabilityAdmissionPolicy {
    executor: ComponentIdentityV1,
    policy: PolicyIdentityV1,
    maximum: CapabilitySetV1,
}

impl CapabilityAdmissionPolicy {
    pub fn new(
        executor: ComponentIdentityV1,
        policy: PolicyIdentityV1,
        maximum: CapabilitySetV1,
    ) -> Self;
}

pub struct AtomicJsonEnvelopeStore {
    root: PathBuf,
}

impl AtomicJsonEnvelopeStore {
    pub fn new(root: impl Into<PathBuf>) -> Self;
}

pub const HTTP_ACTION_NAMESPACE: &str = "http";
pub const HTTP_ACTION_NAME_V1: &str = "request.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpNetworkCeiling {
    LoopbackOnly,
    CapabilityOnly,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpExecutorConfigError {
    #[error("failed to construct bounded HTTP client: {message}")]
    Client { message: String },
}

pub struct HttpActionExecutor {
    executor: ComponentIdentityV1,
    runtime: ComponentIdentityV1,
    evidence_root: PathBuf,
    client: reqwest::Client,
    hard_timeout: Duration,
    hard_response_bytes: u64,
    network_ceiling: HttpNetworkCeiling,
}

impl HttpActionExecutor {
    pub fn new(
        executor: ComponentIdentityV1,
        runtime: ComponentIdentityV1,
        evidence_root: impl Into<PathBuf>,
        network_ceiling: HttpNetworkCeiling,
    ) -> Result<Self, HttpExecutorConfigError>;
}
```

`AtomicJsonEnvelopeStore` uses one `<execution-id>.lock` file with `fs2` exclusive locking around
load, head comparison, temporary-file write, file sync, atomic rename, and parent-directory sync.
CAS is process- and cross-process-safe on filesystems that honor advisory locks.

```rust
#[cfg(feature = "test-utils")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreFaultPoint {
    BeforeTemporaryFileSync,
    BeforeRename,
    BeforeDirectorySync,
}

#[cfg(feature = "test-utils")]
pub trait StoreFaultInjector: Send + Sync {
    fn check(&self, point: StoreFaultPoint) -> std::io::Result<()>;
}

#[cfg(feature = "test-utils")]
impl AtomicJsonEnvelopeStore {
    pub fn with_fault_injector(
        root: impl Into<PathBuf>,
        injector: Arc<dyn StoreFaultInjector>,
    ) -> Self;
}
```

`ed25519-dalek` types remain private. The HTTP executor derives exact scheme, host, and effective
port authorization from admitted `NetworkCapabilityV1`; governed action input contains no
`network_policy` field.

### Feature-Gated Local Server

```rust
#[cfg(feature = "test-utils")]
pub struct LocalHttpServer {
    address: SocketAddr,
    counts: Arc<Mutex<BTreeMap<String, u64>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

#[cfg(feature = "test-utils")]
impl LocalHttpServer {
    pub async fn spawn() -> std::io::Result<Self>;
    pub fn url(&self, path: &str) -> String;
    pub fn request_count(&self, path: &str) -> u64;
    pub async fn shutdown(self) -> std::io::Result<()>;
}

#[cfg(feature = "test-utils")]
pub struct SignedHttpFixture {
    envelope: ExecutionEnvelopeV1,
    key_id: String,
    public_key: [u8; 32],
}

#[cfg(feature = "test-utils")]
impl SignedHttpFixture {
    pub fn envelope(&self) -> &ExecutionEnvelopeV1;
    pub fn into_envelope(self) -> ExecutionEnvelopeV1;
    pub fn key_id(&self) -> &str;
    pub const fn public_key(&self) -> [u8; 32];
}

#[cfg(feature = "test-utils")]
pub fn signed_http_fixture(
    url: &str,
    issued_at: DateTime<Utc>,
) -> Result<SignedHttpFixture, ExecutionEnvelopeError>;

#[cfg(feature = "test-utils")]
pub fn structural_http_fixture(
    url: &str,
    issued_at: DateTime<Utc>,
) -> Result<ExecutionEnvelopeV1, ExecutionEnvelopeError>;
```

The fixture exposes `/execute`, `/fail`, and `/oversized`; responses and counters are deterministic.

## Runtime Ordering And Failure Semantics

1. Validate the `Intent -> Authorization` prefix.
2. Verify statement digest, strict Ed25519 signature, key ID, audience, and issuance/expiry.
3. For deny, review, or retry-later decisions, CAS-store the terminal authorization prefix and
   return the matching outcome without nonce reservation, admission, or action.
4. Reserve the nonce for an executable allow decision.
5. CAS-store the authorized baseline; release the reservation only for conflict or pre-rename I/O
   failure. Retain it for `CommitIndeterminate`.
6. Commit the nonce reservation. A commit failure remains reserved and returns fail-closed.
7. Evaluate admission and append/store `Admission`.
8. Return `AdmissionDenied` without calling the action when denied.
9. Prepare the action and append/store `Environment`.
10. Append/store `Started` from `ExecutionClock::now()`.
11. Call `ActionExecutor::execute`; append/store outputs, artifacts, and one result in order.
12. Validate the terminal envelope and return its status.

Every append is followed by a CAS using the prior durable head. Conflicts and pre-rename failures
restore the mutable envelope to the last durable prefix. Pre-action `CommitIndeterminate` stops
before side effects. Post-action `CommitIndeterminate` preserves the candidate prefix, returns an
error requiring explicit reconciliation, and forbids automatic action retry.

Preparation failures append a failed pre-start result with phase `Preparation`. Failures from
`ActionExecutor::execute` occur after durable `Started` and append a failed result with phase
`Runtime`. The runtime never retries automatically after `Started`; `Spawn` is not emitted by this
two-stage action port.

## HTTP Adapter Semantics

- Action input contains method, URL, headers, optional body, timeout, and response limit.
- Network permission comes exclusively from admitted capabilities.
- `port: None` matches `Url::port_or_known_default()` for the endpoint scheme.
- Effective timeout and total response limits are the minimum of action-requested limits, signed
  resource limits, and executor hard ceilings. `OutputContractV1::max_inline_bytes` limits only
  inline observation data, not externally stored evidence bytes.
- `HttpNetworkCeiling::LoopbackOnly` accepts only literal loopback IP hosts and rejects hostnames or
  non-loopback addresses even when the signed capability permits them; E2E tests and the demo
  always use this mode.
- Redirects are disabled.
- Response bytes are written atomically beneath `evidence_root`; the envelope records a digest,
  byte count, media type, and relative locator.
- Non-success HTTP status is represented in the action observation and does not automatically become
  a transport failure.
- `CapabilityOnly` mode resolves once, validates every candidate address, and pins the selected
  validated socket address into the HTTP client connection. Re-resolution during connection is
  forbidden; multi-request DNS refresh remains out of scope.
- Existing `http::request` remains compatible but delegates bounded transport to a shared private
  implementation.

## E2E Test Matrix

`crates/crux/tests/governed_http.rs` uses `crux-agentic::test_utils::LocalHttpServer`, binding
`127.0.0.1:0` with explicit shutdown and per-endpoint counters.

1. Fixed verifier success: complete lifecycle, one request, persisted prefixes reload and validate.
2. Real Ed25519 success: deterministic signing key, strict signature verification, one request.
3. Altered signature: authorization error, no persisted admission, zero requests.
4. Wrong key, audience, expiry, and repeated nonce: fail before HTTP.
5. Signed deny, review, and retry-later: terminal authorization outcomes with no admission/request.
6. Baseline conflict/pre-rename failure releases nonce; baseline indeterminate and nonce-commit
   failure remain fail-closed.
7. Admission denial/excessive capability: terminal denial, zero requests.
8. Endpoint outside signed allowlist, wrong namespace, wrong action name, or non-loopback hard
   ceiling: zero requests.
9. HTTP transport failure: failed runtime result with intact hash chain.
10. `Started` is durably visible before the server receives the request.
11. Concurrent atomic-store CAS permits exactly one writer.
12. Injected failures before temp sync or rename preserve the previous snapshot; a directory-sync
    failure returns `CommitIndeterminate`. All cases expose only complete old or new JSON, never a
    partial document. Test both pre-action suppression and post-action no-retry reconciliation.
13. Tampered persisted intent/output/identity: digest or authorization validation fails.

The fixed verifier is test support only and validates statement/proof digest bindings without
cryptography. It is never exported in production prelude APIs.

## Runnable Demo

`cargo run -p crux-agentic --example governed_http --features test-utils`:

1. starts the shared local server;
2. writes artifacts and envelope snapshots under `target/governed-http-demo/`;
3. runs signed success, invalid-signature rejection, and admission denial;
4. prints record sequence, kind, digest prefix, lifecycle state, status, evidence locator, and
   request counters;
5. exits nonzero if an unauthorized path reaches the server or a persisted envelope fails validation.

## Data Flow

1. Planner/test code creates and signs an `Intent -> Authorization` envelope.
2. `GovernedExecutor` validates and persists the authorized baseline.
3. `AdmissionPolicy` narrows capabilities and records the local decision.
4. `HttpActionExecutor` prepares environment evidence; runtime durably records `Started`; the
   adapter then executes loopback HTTP.
5. Response bytes flow to evidence storage; references and terminal result flow to the envelope.
6. `EnvelopeStore` CAS-persists every validated prefix for reload and tamper checks.

## Hexagonal Boundaries

- **Authorization port**: `AuthorizationVerifier` in `crux-runtime`.
- **Admission port**: `AdmissionPolicy` in `crux-runtime`.
- **Persistence port**: `EnvelopeStore` in `crux-runtime`.
- **Replay-protection port**: `NonceStore` in `crux-runtime`.
- **Time port**: `ExecutionClock` in `crux-runtime`.
- **Action port**: staged `ActionExecutor` in `crux-runtime`.
- **Adapters**: Ed25519, capability admission, atomic JSON, and HTTP implementations in
  `crux-agentic`.
- **Composition root**: facade E2E and agentic example.

## Out Of Scope

- Automatic envelope creation for every existing `CruxCtx::step`.
- Minibox execution and Pactum transport.
- Durable distributed nonce/key storage.
- Replay-cache or `Emission` integration.
- New CLI commands.
- Public internet fixtures or production DNS rebinding protection.
- Automatic retries after resource release.

## Risk

- [x] Additive public API in `crux-runtime` and `crux-agentic`.
- [x] New `ed25519-dalek` and `fs2` dependencies, isolated behind runtime ports.
- [x] Persisted wire contract prerequisite must retain Rust 1.85 compatibility.
- [x] File-store crash consistency and CAS behavior require failure-injection tests.
- [x] Incorrect capability-to-HTTP mapping could authorize unintended destinations.
- [ ] No breaking change to current `CruxCtx`, handler, replay, or CLI APIs.
- [ ] No remote service, credential, or paid-provider dependency.

## Verification Gates

```text
cargo nextest run -p crux-types --test execution_envelope
cargo nextest run -p crux-runtime governed_execution
cargo nextest run -p crux-agentic --features test-utils http_execution
cargo nextest run -p crux --test governed_http
cargo run -p crux-agentic --example governed_http --features test-utils
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
just ci
```

Also run the independently pinned Rust 1.85 `crux-types` contract gate promised by Execution
Envelope V1.
