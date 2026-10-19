# Design: Execution Envelope V1

## Status

Approved on 2026-09-22. Implementation still requires a separate plan.

## Goal

Define a runtime-neutral, append-only, cryptographically linked execution record that preserves
agent intent, authorization, environment, lifecycle, outputs, verification, and causal identity.

## Approved Approach

Use a Crux-native wire contract in `crux-types`; keep the contract runtime-neutral and use Minibox
as its first concrete executor in separate bounded designs.

## Context Map

### Files To Modify

| File | Purpose | Change |
| --- | --- | --- |
| `Cargo.toml` | Workspace dependencies | Add SHA-256, hexadecimal encoding, JCS canonicalization, and `thiserror`. |
| `crates/crux-types/Cargo.toml` | Wire-type dependencies | Add dependencies and declare a separately tested Rust 1.85 MSRV. |
| `crates/crux-types/src/lib.rs` | Public modules | Export the `execution` module. |
| `crates/crux-types/src/execution/mod.rs` | New contract | Add V1 types, validation, canonical hashing, and signing projection. |
| `crates/crux-types/tests/execution_envelope.rs` | Contract tests | Add round-trip, lifecycle, hash-chain, and signing-vector tests. |

### Dependencies

| File | Relationship |
| --- | --- |
| `crates/crux-types/src/id.rs` | Supplies the existing Crux trace identifier. |
| `crates/crux-types/src/step.rs` | Existing wire-type and Serde conventions. |
| `crates/crux-runtime/src/governance.rs` | Existing policy vocabulary is mapped by adapters, not duplicated. |
| `crates/crux-runtime/src/approval.rs` | Existing approval vocabulary is mapped by adapters. |
| `crates/crux-runtime/src/audit.rs` | A later runtime design may emit records through this port. |

### Risk

- Additive public API in `crux-types`.
- New persisted wire format with strict lifecycle rules.
- Canonical byte changes invalidate record digests and authorization signatures.
- `serde_json::Value` inputs require deterministic recursive object-key ordering.
- Crux uses Rust 1.89 while Minibox uses Rust 1.85; `crux-types` must remain independently
  buildable and tested on Rust 1.85.

## Crate Ownership

- **Owner**: `crux-types` owns transport-neutral V1 values and pure validation.
- **Not owners**: `crux-runtime` does not evaluate authorization in this milestone; executors own
  signature verification and enforcement adapters.
- **Dependency rule**: no Tokio, LLM client, process runtime, or Minibox dependency.

## Wire Model

```text
ExecutionEnvelopeV1
  identity
  records
    0 Intent
    1 Authorization
    2 Admission
    3 Environment
    4 Started
    n Output / Artifact
    n Result
    n Verification
```

A denied, review-required, or retry-later execution terminates at `Authorization`. An externally
authorized execution receives an executor-owned `Admission` record. Admission may deny before any
resource mutation. A successful execution contains `Environment`, `Started`, and exactly one
`Result` in that order; admitted preparation or spawn failures may complete without `Started`.
Every prefix through these states is valid for durable persistence;
`validate_complete()` additionally requires a terminal state. Verification records may follow a
terminal result or terminal denial.

## Public API

Envelope, record, identifier, nonce, and digest fields are private so callers cannot bypass chain
construction. Leaf wire DTO fields are public for adapter ergonomics; the omitted `pub` markers in
the abbreviated field listings are typographic only and implementations must declare those fields
public. All values derive `Debug`,
`Clone`, `PartialEq`, `Eq`, `Serialize`, and `Deserialize`; identifiers and digests also derive
`Hash`. Enums use explicit snake-case Serde names, structs reject unknown fields, and private
values have validated constructors, `FromStr` where useful, and borrowing accessors.

### Identity And Records

```rust
pub const EXECUTION_ENVELOPE_SCHEMA_V1: u32 = 1;

pub struct ExecutionId(String);
pub struct ExecutionRecordId(String);
pub struct AuthorizationNonce(String);

pub enum DigestAlgorithmV1 {
    Sha256,
}

pub struct ContentDigestV1 {
    algorithm: DigestAlgorithmV1,
    value: String,
}

pub struct ActorIdentityV1 {
    kind: ActorKindV1,
    id: String,
}

pub enum ActorKindV1 {
    Agent,
    Human,
    Service,
}

pub struct ExecutionIdentityV1 {
    execution_id: ExecutionId,
    session_id: Option<CruxId>,
    parent_execution_ids: Vec<ExecutionId>,
    actor: ActorIdentityV1,
}

pub struct ExecutionEnvelopeV1 {
    schema_version: u32,
    identity: ExecutionIdentityV1,
    records: Vec<ExecutionRecordV1>,
}

pub struct ExecutionRecordV1 {
    record_id: ExecutionRecordId,
    sequence: u64,
    recorded_at: DateTime<Utc>,
    previous_digest: Option<ContentDigestV1>,
    digest: ContentDigestV1,
    payload: ExecutionRecordPayloadV1,
}

#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ExecutionRecordPayloadV1 {
    Intent(ExecutionIntentV1),
    Authorization(AuthorizationRecordV1),
    Admission(AdmissionRecordV1),
    Environment(ExecutionEnvironmentV1),
    Started(ExecutionStartedV1),
    Output(ExecutionOutputV1),
    Artifact(ArtifactRefV1),
    Result(ExecutionResultV1),
    Verification(VerificationRecordV1),
}
```

Execution IDs use `exec_<26 uppercase ULID>` and record IDs use
`rec_<26 uppercase ULID>`. Nonces are opaque and non-empty. V1 digests require lowercase
hexadecimal SHA-256. Golden vectors freeze identifier casing and prefixes.

### Intent And Capabilities

```rust
pub struct ExecutionIntentV1 {
    action: ActionSpecV1,
    requested_capabilities: CapabilitySetV1,
    expected_output: OutputContractV1,
}

pub struct ActionSpecV1 {
    namespace: String,
    name: String,
    input: serde_json::Value,
    input_schema: Option<ContentDigestV1>,
    side_effect: SideEffectClassV1,
    idempotency_key: Option<String>,
}

pub enum SideEffectClassV1 {
    None,
    ReadOnly,
    Reversible,
    Irreversible,
}

pub struct OutputContractV1 {
    schema: Option<ContentDigestV1>,
    media_types: Vec<String>,
    max_inline_bytes: u64,
}

pub struct CapabilitySetV1 {
    filesystem: Vec<FilesystemScopeV1>,
    network: NetworkCapabilityV1,
    environment_names: Vec<String>,
    secrets: Vec<SecretRefV1>,
    process: ProcessCapabilityV1,
    resources: ResourceLimitsV1,
}

pub struct FilesystemScopeV1 {
    root: String,
    access: FilesystemAccessV1,
}

pub enum FilesystemAccessV1 {
    Read,
    ReadWrite,
}

pub enum NetworkCapabilityV1 {
    Denied,
    AllowList(Vec<NetworkEndpointV1>),
    Unrestricted,
}

pub struct NetworkEndpointV1 {
    scheme: String,
    host: String,
    port: Option<u16>,
}

pub struct SecretRefV1 {
    class: String,
    reference: String,
}

pub struct ProcessCapabilityV1 {
    privileged: bool,
}

pub struct ResourceLimitsV1 {
    wall_time_ms: Option<u64>,
    memory_bytes: Option<u64>,
    cpu_weight: Option<u64>,
    process_count: Option<u64>,
    output_bytes: Option<u64>,
}
```

Secret values never enter the envelope. Capability containment is structural; executors still
perform platform-specific path, symlink, DNS, and runtime checks.

### Authorization

```rust
pub struct AuthorizationRecordV1 {
    statement: AuthorizationStatementV1,
    proof: AuthorizationProofV1,
}

pub struct AuthorizationStatementV1 {
    schema_version: u32,
    execution_identity: ExecutionIdentityV1,
    intent_digest: ContentDigestV1,
    authority: ActorIdentityV1,
    policy: PolicyIdentityV1,
    decision: AuthorizationDecisionV1,
    reasons: Vec<DecisionReasonV1>,
    algorithm: SignatureAlgorithmV1,
    key_id: String,
    audience: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    nonce: AuthorizationNonce,
}

pub struct PolicyIdentityV1 {
    name: String,
    version: String,
    digest: ContentDigestV1,
}

pub enum AuthorizationDecisionV1 {
    Allow { grant: CapabilitySetV1 },
    AllowWithConstraints { grant: CapabilitySetV1 },
    Deny,
    RequireReview { review_id: String },
    RetryLater { not_before: DateTime<Utc> },
}

pub struct DecisionReasonV1 {
    code: String,
    message: String,
}

pub enum SignatureAlgorithmV1 {
    Ed25519,
}

pub struct AuthorizationProofV1 {
    payload_digest: ContentDigestV1,
    signature: String,
}
```

`AuthorizationStatementV1::signing_bytes()` is computed before a proof exists, removing circular
construction. The statement binds the schema, full execution identity, intent digest, authority,
policy, decision, reasons, algorithm, key ID, audience, issuance and expiry times, and nonce. The
proof contains the statement digest and signature only. A grant must be contained by the requested
capability set.

### Executor Admission

```rust
pub struct AdmissionRecordV1 {
    executor: ComponentIdentityV1,
    policy: PolicyIdentityV1,
    decision: AdmissionDecisionV1,
    effective_capabilities: Option<CapabilitySetV1>,
    reasons: Vec<DecisionReasonV1>,
}

pub enum AdmissionDecisionV1 {
    Admitted,
    Denied,
}
```

An admitted effective set must satisfy
`effective capabilities <= signed grant <= requested capabilities`. A denied admission is terminal
and requires no environment or started record.

### Environment, Results, And Verification

```rust
pub struct ComponentIdentityV1 {
    name: String,
    version: String,
    digest: Option<ContentDigestV1>,
}

pub struct ImageIdentityV1 {
    reference: String,
    digest: ContentDigestV1,
}

pub struct EvidenceRefV1 {
    media_type: String,
    digest: ContentDigestV1,
    size_bytes: u64,
    locator: Option<String>,
}

pub struct ExecutionEnvironmentV1 {
    executor: ComponentIdentityV1,
    runtime: ComponentIdentityV1,
    image: Option<ImageIdentityV1>,
    effective_capabilities: CapabilitySetV1,
    native_evidence: Vec<EvidenceRefV1>,
}

pub struct ExecutionStartedV1 {
    started_at: DateTime<Utc>,
}

pub enum OutputStreamV1 {
    Stdout,
    Stderr,
    Structured,
}

pub struct ExecutionOutputV1 {
    stream: OutputStreamV1,
    content: EvidenceRefV1,
    observed_bytes: u64,
    forwarded_bytes: u64,
    truncated: bool,
}

pub struct ArtifactRefV1 {
    name: String,
    evidence: EvidenceRefV1,
}

pub enum ExecutionStatusV1 {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Denied,
}

pub struct ExecutionErrorV1 {
    phase: ExecutionFailurePhaseV1,
    classification: String,
    message: String,
}

pub enum ExecutionFailurePhaseV1 {
    Admission,
    Preparation,
    Spawn,
    Runtime,
    Cleanup,
}

pub struct ReplayMetadataV1 {
    replayable: bool,
    source_execution_id: Option<ExecutionId>,
}

pub struct ExecutionResultV1 {
    status: ExecutionStatusV1,
    completed_at: DateTime<Utc>,
    exit_code: Option<i32>,
    observation: Option<serde_json::Value>,
    error: Option<ExecutionErrorV1>,
    replay: ReplayMetadataV1,
}

pub enum VerificationStatusV1 {
    Passed,
    Failed,
    Inconclusive,
}

pub struct VerificationRecordV1 {
    verifier: ComponentIdentityV1,
    subject_record_id: ExecutionRecordId,
    subject_digest: ContentDigestV1,
    status: VerificationStatusV1,
    checks: Vec<String>,
    evidence: Vec<EvidenceRefV1>,
}
```

### Construction And Validation

```rust
impl ExecutionEnvelopeV1 {
    pub fn new(
        identity: ExecutionIdentityV1,
        intent: ExecutionIntentV1,
        recorded_at: DateTime<Utc>,
    ) -> Result<Self, ExecutionEnvelopeError>;

    pub fn append(
        &mut self,
        payload: ExecutionRecordPayloadV1,
        recorded_at: DateTime<Utc>,
    ) -> Result<&ExecutionRecordV1, ExecutionEnvelopeError>;

    pub fn validate_prefix(&self) -> Result<(), ExecutionEnvelopeError>;
    pub fn validate_complete(&self) -> Result<(), ExecutionEnvelopeError>;
    pub fn lifecycle_state(&self) -> Result<ExecutionLifecycleV1, ExecutionEnvelopeError>;
    pub fn intent(&self) -> Option<&ExecutionIntentV1>;
    pub fn authorization(&self) -> Option<&AuthorizationRecordV1>;
    pub fn identity(&self) -> &ExecutionIdentityV1;
    pub fn records(&self) -> &[ExecutionRecordV1];
    pub fn terminal_status(&self) -> Option<ExecutionStatusV1>;
}

impl AuthorizationStatementV1 {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, ExecutionEnvelopeError>;
    pub fn payload_digest(&self) -> Result<ContentDigestV1, ExecutionEnvelopeError>;
}

impl CapabilitySetV1 {
    pub fn contains(&self, candidate: &Self) -> bool;
}

pub enum ExecutionLifecycleV1 {
    Proposed,
    AuthorizationDenied,
    ReviewRequired,
    RetryLater,
    Authorized,
    AdmissionDenied,
    Admitted,
    Running,
    Completed,
    Verified,
}

pub enum IdentityFieldV1 {
    ExecutionId,
    ActorId,
    ParentExecutionId,
}

pub enum ExecutionEnvelopeError {
    UnsupportedSchema { actual: u32 },
    InvalidIdentity { field: IdentityFieldV1 },
    InvalidRecordOrder { sequence: u64, reason: String },
    InvalidDigest { sequence: u64 },
    BrokenHashChain { sequence: u64 },
    CapabilityEscalation,
    InvalidAuthorization { reason: String },
    InvalidTerminalState { reason: String },
    Canonicalization { reason: String },
}
```

`ExecutionEnvelopeError` derives the same wire traits as the contract and implements
`std::error::Error` through `thiserror`.

## Capability Containment

- Collections are duplicate-free sets; ordering is canonicalized before hashing.
- Filesystem roots are absolute, lexically normalized paths without `..`; a candidate root must be
  equal to or below a requested root, and `Read` is narrower than `ReadWrite`. Executors still
  resolve symlinks and mount semantics.
- `Denied` network is contained by every grant; an allowlist is contained only by a superset
  allowlist or `Unrestricted`; `Unrestricted` is contained only by `Unrestricted`.
- Environment names and secret references use exact set inclusion.
- `privileged: false` is narrower than `true`.
- For resources, `None` means no declared upper bound. A finite candidate is contained by `None`; a
  candidate `None` is not contained by a finite limit; finite candidates must be less than or equal
  to finite limits. CPU weight must be equal because it is a scheduling weight, not a maximum.

## Canonical Bytes

V1 uses RFC 8785 JSON Canonicalization Scheme bytes for dedicated projection structs. Raw JSON is
decoded with duplicate-key rejection before typed deserialization. Timestamps are UTC RFC 3339 with
exactly nine fractional digits. Floating-point values are rejected in action inputs and
observations. Enum spellings are explicit snake case. Ed25519 public keys and signatures use
unpadded base64url; SHA-256 digests use lowercase hexadecimal.

Record hashing is exactly
`SHA256("crux.execution.record.v1\0" || JCS(record_projection_without_digest))`.
Authorization signing bytes are exactly
`"crux.execution.authorization.v1\0" || JCS(authorization_statement)`. The proof payload digest is
the SHA-256 of those signing bytes and is excluded from its own projection. Golden positive and
negative vectors freeze every byte.

The statement `intent_digest` must equal `envelope.records()[0].digest()`. The proof is exactly
`payload_digest = SHA256(signing_bytes)` and
`signature = Ed25519.Sign(private_key, signing_bytes)`; strict verification operates on the signing
bytes, not on a second signature over `payload_digest`.

`validate_prefix()` also requires the statement execution identity to equal the envelope identity
exactly. Golden negative vectors cover actor, session, parent, and execution-ID substitution.

## Validation Ceilings

- Serialized envelope: 1 MiB; records: 256; parents: 16.
- String value: 4 KiB; action input: 256 KiB; JSON nesting: 32.
- Reasons: 32; filesystem scopes: 64; network endpoints: 64; evidence refs: 64.
- Record timestamps are nondecreasing; issue time is not after expiry; start is not after completion.
- Record IDs are unique; execution IDs cannot be their own parent; parent IDs are sorted.
- Canonical numeric values are limited to `0..=9_007_199_254_740_991`; action inputs and
  observations containing floats, negative values, or larger integers are rejected.
- `VerificationRecordV1` must reference an earlier terminal record whose ID and digest both match;
  forward, missing, nonterminal, or mismatched references are rejected.
- `Admission::Admitted` requires effective capabilities, `Admission::Denied` forbids them, and an
  environment record repeats the admitted effective capability set exactly.
- `Succeeded` requires a prior `Started` record. `Failed`, `Cancelled`, or `TimedOut` may terminate
  an admitted or environment-prepared prefix before `Started` and identify the failure phase.

## Data Flow

1. A planner creates an intent-only envelope.
2. An external authority appends a signed authorization record.
3. An executor validates it and appends an admission decision.
4. An admitted executor appends environment, lifecycle, output, artifacts, and one result.
5. Independent verifiers append verification records without modifying prior evidence.

## Out Of Scope

- Policy evaluation, key storage, signature verification, nonce persistence, and execution.
- Pactum projection or generic transport protocol.
- `CruxCtx`, replay-cache, or `Emission` integration.
- Inline secrets or unbounded stdout/stderr payloads.
- Tamper resistance against a compromised executor or host root; V1 trusts the executor and uses
  the hash chain against accidental or off-path modification.
- Authentication of intent-only envelopes before authorization. The external authorization
  signature binds the full identity and intent digest and is the first authenticated chain point.
- Cryptographic authentication of post-execution verification claims. V1 records verifier identity,
  subject digest, checks, and evidence under the trusted executor/store threat model; a later schema
  version may add verifier signatures.

## Acceptance Criteria

- Golden fixtures round-trip byte-for-byte.
- Changed records, broken digests, sequence gaps, duplicate terminal records, and capability
  escalation fail validation.
- Deny, review-required, and retry-later envelopes validate without execution records.
- Every lifecycle prefix validates; successful envelopes require environment, start, and result
  records, while admitted preparation and spawn failures may complete before start.
- Local admission denial is terminal before allocation and identifies the denying policy.
- Signing bytes are stable across repeated construction and map insertion order.
- `crux-types` passes contract tests on Rust 1.85 and the workspace toolchain.
