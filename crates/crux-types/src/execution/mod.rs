//! Versioned, transport-neutral governed execution records.

use std::{fmt, str::FromStr};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, Utc};
use jcs_admit::{Options as JcsOptions, admit_with};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use ulid::Ulid;

use crate::id::CruxId;

/// Schema version for the first execution envelope wire contract.
pub const EXECUTION_ENVELOPE_SCHEMA_V1: u32 = 1;

const EXECUTION_ID_PREFIX: &str = "exec_";
const RECORD_ID_PREFIX: &str = "rec_";
const ULID_TEXT_LENGTH: usize = 26;
const SHA256_HEX_LENGTH: usize = 64;
const MAX_STRING_LENGTH: usize = 4096;
const MAX_PARENT_EXECUTIONS: usize = 16;
const MAX_COLLECTION_LENGTH: usize = 256;
const MAX_FILESYSTEM_SCOPES: usize = 64;
const MAX_NETWORK_ENDPOINTS: usize = 64;
const MAX_EVIDENCE_REFS: usize = 64;
const MAX_DECISION_REASONS: usize = 32;
const MAX_RECORDS: usize = 256;
const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;

/// Identifies an invalid execution identity field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityFieldV1 {
    /// The execution identifier.
    ExecutionId,
    /// The actor identifier.
    ActorId,
    /// A parent execution identifier.
    ParentExecutionId,
    /// An execution record identifier.
    RecordId,
}

/// Validation error for execution envelope values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionEnvelopeError {
    /// An identifier did not use the required prefix and uppercase ULID form.
    #[error("invalid {field:?}")]
    InvalidIdentity {
        /// Field that failed validation.
        field: IdentityFieldV1,
    },
    /// An authorization nonce was empty, padded, or too long.
    #[error("invalid authorization nonce")]
    InvalidNonce,
    /// A digest was not lowercase SHA-256 hexadecimal.
    #[error("invalid SHA-256 digest")]
    InvalidDigestValue,
    /// An action field or typed JSON input was invalid.
    #[error("invalid action field '{field}': {reason}")]
    InvalidAction {
        /// Stable field name.
        field: String,
        /// Human-readable validation reason.
        reason: String,
    },
    /// A capability field violated a normalization or containment invariant.
    #[error("invalid capability field '{field}': {reason}")]
    InvalidCapability {
        /// Stable field name.
        field: String,
        /// Human-readable validation reason.
        reason: String,
    },
    /// Raw or projected JSON could not be admitted or canonicalized.
    #[error("canonicalization failed: {reason}")]
    Canonicalization {
        /// Human-readable canonicalization failure.
        reason: String,
    },
    /// A record digest did not match its canonical projection.
    #[error("record {sequence} has an invalid digest")]
    InvalidDigest {
        /// Record sequence whose digest failed.
        sequence: u64,
    },
    /// A record field violated the V1 wire contract.
    #[error("record {sequence} is invalid: {reason}")]
    InvalidRecord {
        /// Record sequence that failed validation.
        sequence: u64,
        /// Human-readable validation reason.
        reason: String,
    },
    /// An authorization statement or proof was invalid.
    #[error("invalid authorization: {reason}")]
    InvalidAuthorization {
        /// Human-readable validation reason.
        reason: String,
    },
    /// The envelope schema version is not supported.
    #[error("unsupported execution envelope schema version {actual}")]
    UnsupportedSchema {
        /// Unsupported schema version.
        actual: u32,
    },
    /// Record order or lifecycle state is invalid.
    #[error("invalid record order at sequence {sequence}: {reason}")]
    InvalidRecordOrder {
        /// Sequence at which validation failed.
        sequence: u64,
        /// Human-readable ordering reason.
        reason: String,
    },
    /// A record does not link to the preceding digest.
    #[error("broken hash chain at sequence {sequence}")]
    BrokenHashChain {
        /// Sequence at which the chain broke.
        sequence: u64,
    },
    /// A policy grant or effective capability set exceeds its parent set.
    #[error("capability escalation")]
    CapabilityEscalation,
    /// The envelope is not in a valid terminal state.
    #[error("invalid terminal state: {reason}")]
    InvalidTerminalState {
        /// Human-readable terminal-state reason.
        reason: String,
    },
}

/// Unique identifier for one governed execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ExecutionId(String);

impl ExecutionId {
    /// Generates an `exec_`-prefixed uppercase ULID.
    #[must_use]
    pub fn new() -> Self {
        Self(format!("{EXECUTION_ID_PREFIX}{}", Ulid::new()))
    }

    /// Borrows the complete wire identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ExecutionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ExecutionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ExecutionId {
    type Err = ExecutionEnvelopeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_prefixed_ulid(value, EXECUTION_ID_PREFIX, IdentityFieldV1::ExecutionId)?;
        Ok(Self(value.to_string()))
    }
}

impl<'de> Deserialize<'de> for ExecutionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(de::Error::custom)
    }
}

/// Unique identifier for one record in an execution envelope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ExecutionRecordId(String);

impl ExecutionRecordId {
    /// Generates a `rec_`-prefixed uppercase ULID.
    #[must_use]
    pub fn new() -> Self {
        Self(format!("{RECORD_ID_PREFIX}{}", Ulid::new()))
    }

    /// Borrows the complete wire identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ExecutionRecordId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ExecutionRecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ExecutionRecordId {
    type Err = ExecutionEnvelopeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_prefixed_ulid(value, RECORD_ID_PREFIX, IdentityFieldV1::RecordId)?;
        Ok(Self(value.to_string()))
    }
}

impl<'de> Deserialize<'de> for ExecutionRecordId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(de::Error::custom)
    }
}

/// Opaque, single-use value included in an authorization statement.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct AuthorizationNonce(String);

impl AuthorizationNonce {
    /// Creates a validated opaque nonce.
    pub fn new(value: impl Into<String>) -> Result<Self, ExecutionEnvelopeError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_STRING_LENGTH || value.trim() != value {
            return Err(ExecutionEnvelopeError::InvalidNonce);
        }
        Ok(Self(value))
    }

    /// Borrows the nonce value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AuthorizationNonce {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Hash algorithm used by V1 content digests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestAlgorithmV1 {
    /// SHA-256.
    Sha256,
}

/// Content digest used for records, evidence, policies, and signed payloads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ContentDigestV1 {
    algorithm: DigestAlgorithmV1,
    value: String,
}

impl ContentDigestV1 {
    /// Creates a validated SHA-256 digest from lowercase hexadecimal.
    pub fn sha256(value: impl Into<String>) -> Result<Self, ExecutionEnvelopeError> {
        let value = value.into();
        if value.len() != SHA256_HEX_LENGTH
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ExecutionEnvelopeError::InvalidDigestValue);
        }
        Ok(Self {
            algorithm: DigestAlgorithmV1::Sha256,
            value,
        })
    }

    /// Computes a SHA-256 digest for bytes.
    #[must_use]
    pub fn digest(bytes: &[u8]) -> Self {
        Self {
            algorithm: DigestAlgorithmV1::Sha256,
            value: hex::encode(Sha256::digest(bytes)),
        }
    }

    /// Returns the digest algorithm.
    #[must_use]
    pub const fn algorithm(&self) -> DigestAlgorithmV1 {
        self.algorithm
    }

    /// Borrows the lowercase hexadecimal digest value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl<'de> Deserialize<'de> for ContentDigestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct DigestWire {
            algorithm: DigestAlgorithmV1,
            value: String,
        }

        let wire = DigestWire::deserialize(deserializer)?;
        match wire.algorithm {
            DigestAlgorithmV1::Sha256 => Self::sha256(wire.value).map_err(de::Error::custom),
        }
    }
}

/// Kind of actor that requested an execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKindV1 {
    /// An autonomous or model-backed agent.
    Agent,
    /// A human operator.
    Human,
    /// A software service.
    Service,
}

/// Stable identity for the actor requesting an execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ActorIdentityV1 {
    kind: ActorKindV1,
    id: String,
}

impl ActorIdentityV1 {
    /// Creates a validated actor identity.
    pub fn new(kind: ActorKindV1, id: impl Into<String>) -> Result<Self, ExecutionEnvelopeError> {
        let id = id.into();
        if id.is_empty() || id.len() > MAX_STRING_LENGTH || id.trim() != id {
            return Err(ExecutionEnvelopeError::InvalidIdentity {
                field: IdentityFieldV1::ActorId,
            });
        }
        Ok(Self { kind, id })
    }

    /// Returns the actor kind.
    #[must_use]
    pub const fn kind(&self) -> ActorKindV1 {
        self.kind
    }

    /// Borrows the stable actor identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl<'de> Deserialize<'de> for ActorIdentityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ActorWire {
            kind: ActorKindV1,
            id: String,
        }

        let wire = ActorWire::deserialize(deserializer)?;
        Self::new(wire.kind, wire.id).map_err(de::Error::custom)
    }
}

/// Identity and causal parents shared by every record in an execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionIdentityV1 {
    execution_id: ExecutionId,
    session_id: Option<CruxId>,
    parent_execution_ids: Vec<ExecutionId>,
    actor: ActorIdentityV1,
}

impl ExecutionIdentityV1 {
    /// Creates a validated execution identity.
    pub fn new(
        execution_id: ExecutionId,
        session_id: Option<CruxId>,
        parent_execution_ids: Vec<ExecutionId>,
        actor: ActorIdentityV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        if parent_execution_ids.len() > MAX_PARENT_EXECUTIONS {
            return Err(ExecutionEnvelopeError::InvalidIdentity {
                field: IdentityFieldV1::ParentExecutionId,
            });
        }
        if parent_execution_ids
            .iter()
            .any(|parent| parent == &execution_id)
            || parent_execution_ids
                .windows(2)
                .any(|pair| pair[0].as_str() >= pair[1].as_str())
        {
            return Err(ExecutionEnvelopeError::InvalidIdentity {
                field: IdentityFieldV1::ParentExecutionId,
            });
        }
        Ok(Self {
            execution_id,
            session_id,
            parent_execution_ids,
            actor,
        })
    }

    /// Borrows the execution identifier.
    #[must_use]
    pub const fn execution_id(&self) -> &ExecutionId {
        &self.execution_id
    }

    /// Borrows the optional Crux session identifier.
    #[must_use]
    pub const fn session_id(&self) -> Option<&CruxId> {
        self.session_id.as_ref()
    }

    /// Borrows causal parent identifiers.
    #[must_use]
    pub fn parent_execution_ids(&self) -> &[ExecutionId] {
        &self.parent_execution_ids
    }

    /// Borrows the requesting actor identity.
    #[must_use]
    pub const fn actor(&self) -> &ActorIdentityV1 {
        &self.actor
    }
}

impl<'de> Deserialize<'de> for ExecutionIdentityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct IdentityWire {
            execution_id: ExecutionId,
            session_id: Option<CruxId>,
            parent_execution_ids: Vec<ExecutionId>,
            actor: ActorIdentityV1,
        }

        let wire = IdentityWire::deserialize(deserializer)?;
        Self::new(
            wire.execution_id,
            wire.session_id,
            wire.parent_execution_ids,
            wire.actor,
        )
        .map_err(de::Error::custom)
    }
}

/// Side-effect classification for a requested action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectClassV1 {
    /// The action has no observable external side effects.
    None,
    /// The action reads external state without mutating it.
    ReadOnly,
    /// The action mutates state and has a defined reversal operation.
    Reversible,
    /// The action may cause an irreversible external mutation.
    Irreversible,
}

/// Runtime-neutral action selected by an agent planner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionSpecV1 {
    /// Stable action namespace.
    pub namespace: String,
    /// Versioned action name.
    pub name: String,
    /// Typed action input.
    pub input: serde_json::Value,
    /// Optional digest of the input schema.
    pub input_schema: Option<ContentDigestV1>,
    /// Side-effect classification.
    pub side_effect: SideEffectClassV1,
    /// Optional caller-selected idempotency key.
    pub idempotency_key: Option<String>,
}

impl ActionSpecV1 {
    /// Creates a validated action specification.
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        input: serde_json::Value,
        input_schema: Option<ContentDigestV1>,
        side_effect: SideEffectClassV1,
        idempotency_key: Option<String>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let namespace = namespace.into();
        let name = name.into();
        validate_nonempty_string("namespace", &namespace)?;
        validate_nonempty_string("name", &name)?;
        if let Some(key) = &idempotency_key {
            validate_nonempty_string("idempotency_key", key)?;
        }
        validate_json(&input)?;
        Ok(Self {
            namespace,
            name,
            input,
            input_schema,
            side_effect,
            idempotency_key,
        })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        let canonical = Self::new(
            self.namespace.clone(),
            self.name.clone(),
            self.input.clone(),
            self.input_schema.clone(),
            self.side_effect,
            self.idempotency_key.clone(),
        )?;
        if canonical != *self {
            return Err(invalid_action("action", "action is not canonical"));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ActionSpecV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ActionWire {
            namespace: String,
            name: String,
            input: UniqueJsonValue,
            input_schema: Option<ContentDigestV1>,
            side_effect: SideEffectClassV1,
            idempotency_key: Option<String>,
        }

        let wire = ActionWire::deserialize(deserializer)?;
        Self::new(
            wire.namespace,
            wire.name,
            wire.input.0,
            wire.input_schema,
            wire.side_effect,
            wire.idempotency_key,
        )
        .map_err(de::Error::custom)
    }
}

/// Contract describing the expected action output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputContractV1 {
    /// Optional digest of the output schema.
    pub schema: Option<ContentDigestV1>,
    /// Accepted output media types in canonical order.
    pub media_types: Vec<String>,
    /// Maximum number of bytes that may be stored inline.
    pub max_inline_bytes: u64,
}

impl OutputContractV1 {
    /// Creates a validated output contract.
    pub fn new(
        schema: Option<ContentDigestV1>,
        mut media_types: Vec<String>,
        max_inline_bytes: u64,
    ) -> Result<Self, ExecutionEnvelopeError> {
        if media_types.len() > MAX_COLLECTION_LENGTH {
            return Err(invalid_action("media_types", "more than 256 media types"));
        }
        validate_safe_integer("max_inline_bytes", max_inline_bytes)?;
        for media_type in &media_types {
            validate_nonempty_string("media_types", media_type)?;
        }
        sort_unique_strings("media_types", &mut media_types)?;
        Ok(Self {
            schema,
            media_types,
            max_inline_bytes,
        })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        let canonical = Self::new(
            self.schema.clone(),
            self.media_types.clone(),
            self.max_inline_bytes,
        )?;
        if canonical != *self {
            return Err(invalid_action(
                "expected_output",
                "output contract is not canonical",
            ));
        }
        Ok(())
    }

    /// Returns the maximum inline output size.
    #[must_use]
    pub const fn max_inline_bytes(&self) -> u64 {
        self.max_inline_bytes
    }
}

impl<'de> Deserialize<'de> for OutputContractV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct OutputWire {
            schema: Option<ContentDigestV1>,
            media_types: Vec<String>,
            max_inline_bytes: u64,
        }

        let wire = OutputWire::deserialize(deserializer)?;
        Self::new(wire.schema, wire.media_types, wire.max_inline_bytes).map_err(de::Error::custom)
    }
}

/// Filesystem access level granted beneath a scope root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemAccessV1 {
    /// Read-only access.
    Read,
    /// Read and write access.
    ReadWrite,
}

/// Normalized absolute filesystem scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct FilesystemScopeV1 {
    /// Absolute normalized root path.
    pub root: String,
    /// Maximum access beneath the root.
    pub access: FilesystemAccessV1,
}

impl FilesystemScopeV1 {
    /// Creates a validated filesystem scope.
    pub fn new(
        root: impl Into<String>,
        access: FilesystemAccessV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let root = root.into();
        if !is_normalized_absolute_path(&root) {
            return Err(invalid_capability(
                "filesystem.root",
                "path must be absolute and lexically normalized",
            ));
        }
        Ok(Self { root, access })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(self.root.clone(), self.access).map(|_| ())
    }

    fn contains(&self, candidate: &Self) -> bool {
        let path_contained = if self.root == "/" {
            candidate.root.starts_with('/')
        } else {
            candidate.root == self.root
                || candidate
                    .root
                    .strip_prefix(&self.root)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        };
        path_contained
            && (self.access == FilesystemAccessV1::ReadWrite
                || candidate.access == FilesystemAccessV1::Read)
    }
}

impl<'de> Deserialize<'de> for FilesystemScopeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ScopeWire {
            root: String,
            access: FilesystemAccessV1,
        }

        let wire = ScopeWire::deserialize(deserializer)?;
        Self::new(wire.root, wire.access).map_err(de::Error::custom)
    }
}

/// One exact network endpoint in an allowlist.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct NetworkEndpointV1 {
    /// Lowercase URI scheme.
    pub scheme: String,
    /// Lowercase host name or literal address.
    pub host: String,
    /// Optional exact port.
    pub port: Option<u16>,
}

impl NetworkEndpointV1 {
    /// Creates a validated exact endpoint.
    pub fn new(
        scheme: impl Into<String>,
        host: impl Into<String>,
        port: Option<u16>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let scheme = scheme.into();
        let host = host.into();
        validate_nonempty_string("network.scheme", &scheme)?;
        validate_nonempty_string("network.host", &host)?;
        let scheme_is_valid = scheme.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || (index > 0 && (byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')))
        });
        let host_is_valid = host.is_ascii()
            && !host.bytes().any(|byte| {
                byte.is_ascii_whitespace()
                    || byte.is_ascii_control()
                    || matches!(byte, b'@' | b'?' | b'#' | b'\\' | b'/')
            });
        if !scheme_is_valid
            || !host_is_valid
            || scheme.is_empty()
            || host.is_empty()
            || host != host.to_ascii_lowercase()
        {
            return Err(invalid_capability(
                "network.endpoint",
                "scheme and host must be unambiguous lowercase ASCII authority values",
            ));
        }
        Ok(Self { scheme, host, port })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(self.scheme.clone(), self.host.clone(), self.port).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for NetworkEndpointV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EndpointWire {
            scheme: String,
            host: String,
            port: Option<u16>,
        }

        let wire = EndpointWire::deserialize(deserializer)?;
        Self::new(wire.scheme, wire.host, wire.port).map_err(de::Error::custom)
    }
}

/// Network authority granted to an execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkCapabilityV1 {
    /// No network access.
    Denied,
    /// Access only to exact listed endpoints.
    AllowList(Vec<NetworkEndpointV1>),
    /// Unrestricted network access.
    Unrestricted,
}

impl NetworkCapabilityV1 {
    /// Creates a canonical duplicate-free endpoint allowlist.
    pub fn allow_list(
        mut endpoints: Vec<NetworkEndpointV1>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        if endpoints.len() > MAX_NETWORK_ENDPOINTS {
            return Err(invalid_capability(
                "network.allow_list",
                "more than 64 endpoints",
            ));
        }
        endpoints.sort();
        if endpoints.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid_capability(
                "network.allow_list",
                "duplicate endpoint",
            ));
        }
        Ok(Self::AllowList(endpoints))
    }

    /// Reports whether this network grant contains a candidate grant.
    #[must_use]
    pub fn contains(&self, candidate: &Self) -> bool {
        match (self, candidate) {
            (_, Self::Denied) | (Self::Unrestricted, _) => true,
            (Self::AllowList(allowed), Self::AllowList(candidate)) => {
                candidate.iter().all(|endpoint| allowed.contains(endpoint))
            }
            (Self::Denied, Self::AllowList(_) | Self::Unrestricted)
            | (Self::AllowList(_), Self::Unrestricted) => false,
        }
    }

    fn normalized(self) -> Result<Self, ExecutionEnvelopeError> {
        match self {
            Self::AllowList(endpoints) => Self::allow_list(endpoints),
            other => Ok(other),
        }
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        let canonical = self.clone().normalized()?;
        if canonical != *self {
            return Err(invalid_capability(
                "network",
                "network capability is not canonical",
            ));
        }
        for endpoint in match self {
            Self::AllowList(endpoints) => endpoints.as_slice(),
            Self::Denied | Self::Unrestricted => &[],
        } {
            endpoint.validate()?;
        }
        Ok(())
    }
}

/// Opaque reference to a secret that may be resolved by an executor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct SecretRefV1 {
    /// Executor-defined secret class.
    pub class: String,
    /// Opaque secret reference; never the secret value.
    pub reference: String,
}

impl SecretRefV1 {
    /// Creates a validated opaque secret reference.
    pub fn new(
        class: impl Into<String>,
        reference: impl Into<String>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let class = class.into();
        let reference = reference.into();
        validate_nonempty_string("secret.class", &class)?;
        validate_nonempty_string("secret.reference", &reference)?;
        let Some((scheme, opaque)) = reference.split_once("://") else {
            return Err(invalid_capability(
                "secret.reference",
                "must be an opaque resolver URI",
            ));
        };
        if scheme.is_empty()
            || opaque.is_empty()
            || !scheme.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase()
                    || (index > 0 && (byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')))
            })
            || reference
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return Err(invalid_capability(
                "secret.reference",
                "must be a lowercase opaque resolver URI without whitespace",
            ));
        }
        Ok(Self { class, reference })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(self.class.clone(), self.reference.clone()).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for SecretRefV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SecretWire {
            class: String,
            reference: String,
        }

        let wire = SecretWire::deserialize(deserializer)?;
        Self::new(wire.class, wire.reference).map_err(de::Error::custom)
    }
}

/// Process privilege authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessCapabilityV1 {
    /// Whether privileged process execution is permitted.
    pub privileged: bool,
}

impl ProcessCapabilityV1 {
    /// Creates a process capability.
    #[must_use]
    pub const fn new(privileged: bool) -> Self {
        Self { privileged }
    }

    const fn contains(self, candidate: Self) -> bool {
        self.privileged || !candidate.privileged
    }
}

/// Upper bounds on execution resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ResourceLimitsV1 {
    /// Wall-clock execution limit in milliseconds.
    pub wall_time_ms: Option<u64>,
    /// Memory limit in bytes.
    pub memory_bytes: Option<u64>,
    /// Exact relative CPU scheduling weight.
    pub cpu_weight: Option<u64>,
    /// Process-count limit.
    pub process_count: Option<u64>,
    /// Output byte limit.
    pub output_bytes: Option<u64>,
}

impl ResourceLimitsV1 {
    /// Creates validated resource limits.
    pub fn new(
        wall_time_ms: Option<u64>,
        memory_bytes: Option<u64>,
        cpu_weight: Option<u64>,
        process_count: Option<u64>,
        output_bytes: Option<u64>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        for (field, value) in [
            ("resources.wall_time_ms", wall_time_ms),
            ("resources.memory_bytes", memory_bytes),
            ("resources.cpu_weight", cpu_weight),
            ("resources.process_count", process_count),
            ("resources.output_bytes", output_bytes),
        ] {
            if let Some(value) = value {
                validate_safe_integer(field, value)?;
            }
        }
        Ok(Self {
            wall_time_ms,
            memory_bytes,
            cpu_weight,
            process_count,
            output_bytes,
        })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(
            self.wall_time_ms,
            self.memory_bytes,
            self.cpu_weight,
            self.process_count,
            self.output_bytes,
        )
        .map(|_| ())
    }

    /// Reports whether these limits contain a candidate set.
    #[must_use]
    pub fn contains(&self, candidate: &Self) -> bool {
        limit_contains(self.wall_time_ms, candidate.wall_time_ms)
            && limit_contains(self.memory_bytes, candidate.memory_bytes)
            && exact_contains(self.cpu_weight, candidate.cpu_weight)
            && limit_contains(self.process_count, candidate.process_count)
            && limit_contains(self.output_bytes, candidate.output_bytes)
    }
}

impl<'de> Deserialize<'de> for ResourceLimitsV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ResourcesWire {
            wall_time_ms: Option<u64>,
            memory_bytes: Option<u64>,
            cpu_weight: Option<u64>,
            process_count: Option<u64>,
            output_bytes: Option<u64>,
        }

        let wire = ResourcesWire::deserialize(deserializer)?;
        Self::new(
            wire.wall_time_ms,
            wire.memory_bytes,
            wire.cpu_weight,
            wire.process_count,
            wire.output_bytes,
        )
        .map_err(de::Error::custom)
    }
}

/// Complete capability request or grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilitySetV1 {
    filesystem: Vec<FilesystemScopeV1>,
    network: NetworkCapabilityV1,
    environment_names: Vec<String>,
    secrets: Vec<SecretRefV1>,
    process: ProcessCapabilityV1,
    resources: ResourceLimitsV1,
}

impl CapabilitySetV1 {
    /// Creates a canonical validated capability set.
    pub fn new(
        mut filesystem: Vec<FilesystemScopeV1>,
        network: NetworkCapabilityV1,
        mut environment_names: Vec<String>,
        mut secrets: Vec<SecretRefV1>,
        process: ProcessCapabilityV1,
        resources: ResourceLimitsV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        if filesystem.len() > MAX_FILESYSTEM_SCOPES {
            return Err(invalid_capability(
                "filesystem",
                "more than 64 filesystem scopes",
            ));
        }
        if environment_names.len() > MAX_COLLECTION_LENGTH {
            return Err(invalid_capability(
                "environment_names",
                "more than 256 environment names",
            ));
        }
        if secrets.len() > MAX_COLLECTION_LENGTH {
            return Err(invalid_capability(
                "secrets",
                "more than 256 secret references",
            ));
        }
        for scope in &filesystem {
            scope.validate()?;
        }
        network.validate()?;
        for secret in &secrets {
            secret.validate()?;
        }
        resources.validate()?;
        filesystem.sort();
        if filesystem
            .windows(2)
            .any(|pair| pair[0].root == pair[1].root)
        {
            return Err(invalid_capability("filesystem", "duplicate scope root"));
        }
        for name in &environment_names {
            validate_nonempty_string("environment_names", name)?;
        }
        sort_unique_strings("environment_names", &mut environment_names)?;
        secrets.sort();
        if secrets.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid_capability("secrets", "duplicate secret reference"));
        }
        Ok(Self {
            filesystem,
            network: network.normalized()?,
            environment_names,
            secrets,
            process,
            resources,
        })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        for scope in &self.filesystem {
            scope.validate()?;
        }
        self.network.validate()?;
        for secret in &self.secrets {
            secret.validate()?;
        }
        self.resources.validate()?;
        let canonical = Self::new(
            self.filesystem.clone(),
            self.network.clone(),
            self.environment_names.clone(),
            self.secrets.clone(),
            self.process,
            self.resources.clone(),
        )?;
        if canonical != *self {
            return Err(invalid_capability(
                "capability_set",
                "capability set is not canonical",
            ));
        }
        Ok(())
    }

    /// Reports whether this capability set contains a candidate grant.
    #[must_use]
    pub fn contains(&self, candidate: &Self) -> bool {
        candidate.filesystem.iter().all(|candidate_scope| {
            self.filesystem
                .iter()
                .any(|scope| scope.contains(candidate_scope))
        }) && self.network.contains(&candidate.network)
            && candidate
                .environment_names
                .iter()
                .all(|name| self.environment_names.contains(name))
            && candidate
                .secrets
                .iter()
                .all(|secret| self.secrets.contains(secret))
            && self.process.contains(candidate.process)
            && self.resources.contains(&candidate.resources)
    }

    /// Borrows filesystem scopes.
    #[must_use]
    pub fn filesystem(&self) -> &[FilesystemScopeV1] {
        &self.filesystem
    }

    /// Borrows the network capability.
    #[must_use]
    pub const fn network(&self) -> &NetworkCapabilityV1 {
        &self.network
    }

    /// Borrows allowed environment variable names.
    #[must_use]
    pub fn environment_names(&self) -> &[String] {
        &self.environment_names
    }

    /// Borrows secret references.
    #[must_use]
    pub fn secrets(&self) -> &[SecretRefV1] {
        &self.secrets
    }

    /// Returns process privilege authority.
    #[must_use]
    pub const fn process(&self) -> ProcessCapabilityV1 {
        self.process
    }

    /// Borrows resource limits.
    #[must_use]
    pub const fn resources(&self) -> &ResourceLimitsV1 {
        &self.resources
    }
}

impl<'de> Deserialize<'de> for CapabilitySetV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CapabilityWire {
            filesystem: Vec<FilesystemScopeV1>,
            network: NetworkCapabilityV1,
            environment_names: Vec<String>,
            secrets: Vec<SecretRefV1>,
            process: ProcessCapabilityV1,
            resources: ResourceLimitsV1,
        }

        let wire = CapabilityWire::deserialize(deserializer)?;
        Self::new(
            wire.filesystem,
            wire.network,
            wire.environment_names,
            wire.secrets,
            wire.process,
            wire.resources,
        )
        .map_err(de::Error::custom)
    }
}

/// Intent record payload created before authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionIntentV1 {
    /// Requested action.
    pub action: ActionSpecV1,
    /// Maximum authority requested by the action.
    pub requested_capabilities: CapabilitySetV1,
    /// Expected output contract.
    pub expected_output: OutputContractV1,
}

impl ExecutionIntentV1 {
    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        self.action.validate()?;
        self.requested_capabilities.validate()?;
        self.expected_output.validate()
    }
}

/// Immutable identity of the policy that made an authorization decision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct PolicyIdentityV1 {
    name: String,
    version: String,
    digest: ContentDigestV1,
}

impl PolicyIdentityV1 {
    /// Creates a validated policy identity.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        digest: ContentDigestV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let name = name.into();
        let version = version.into();
        validate_nonempty_string("policy.name", &name)?;
        validate_nonempty_string("policy.version", &version)?;
        Ok(Self {
            name,
            version,
            digest,
        })
    }

    /// Borrows the policy name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrows the policy version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Borrows the policy content digest.
    #[must_use]
    pub const fn digest(&self) -> &ContentDigestV1 {
        &self.digest
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(self.name.clone(), self.version.clone(), self.digest.clone()).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for PolicyIdentityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PolicyWire {
            name: String,
            version: String,
            digest: ContentDigestV1,
        }

        let wire = PolicyWire::deserialize(deserializer)?;
        Self::new(wire.name, wire.version, wire.digest).map_err(de::Error::custom)
    }
}

/// Stable coded explanation attached to an authorization decision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct DecisionReasonV1 {
    code: String,
    message: String,
}

impl DecisionReasonV1 {
    /// Creates a validated decision reason.
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let code = code.into();
        let message = message.into();
        validate_nonempty_string("reason.code", &code)?;
        validate_nonempty_string("reason.message", &message)?;
        Ok(Self { code, message })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(self.code.clone(), self.message.clone()).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for DecisionReasonV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ReasonWire {
            code: String,
            message: String,
        }

        let wire = ReasonWire::deserialize(deserializer)?;
        Self::new(wire.code, wire.message).map_err(de::Error::custom)
    }
}

/// Policy disposition for a proposed execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationDecisionV1 {
    /// Execute under the supplied grant.
    Allow {
        /// Authorized capability set.
        grant: CapabilitySetV1,
    },
    /// Execute only under a grant narrower than requested.
    AllowWithConstraints {
        /// Authorized constrained capability set.
        grant: CapabilitySetV1,
    },
    /// Do not execute.
    Deny,
    /// Pause until a named review completes.
    RequireReview {
        /// Stable review request identifier.
        review_id: String,
    },
    /// Retry no earlier than a specified instant.
    RetryLater {
        /// Earliest permitted retry instant.
        not_before: DateTime<Utc>,
    },
}

impl AuthorizationDecisionV1 {
    /// Borrows the executable capability grant, if present.
    #[must_use]
    pub const fn grant(&self) -> Option<&CapabilitySetV1> {
        match self {
            Self::Allow { grant } | Self::AllowWithConstraints { grant } => Some(grant),
            Self::Deny | Self::RequireReview { .. } | Self::RetryLater { .. } => None,
        }
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        match self {
            Self::Allow { grant } | Self::AllowWithConstraints { grant } => grant.validate(),
            Self::Deny => Ok(()),
            Self::RequireReview { review_id } => {
                validate_nonempty_string("authorization.review_id", review_id)
            }
            Self::RetryLater { .. } => Ok(()),
        }
    }
}

impl Serialize for AuthorizationDecisionV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match self {
            Self::Allow { grant } => AuthorizationDecisionWireRef::Allow { grant },
            Self::AllowWithConstraints { grant } => {
                AuthorizationDecisionWireRef::AllowWithConstraints { grant }
            }
            Self::Deny => AuthorizationDecisionWireRef::Deny,
            Self::RequireReview { review_id } => {
                AuthorizationDecisionWireRef::RequireReview { review_id }
            }
            Self::RetryLater { not_before } => AuthorizationDecisionWireRef::RetryLater {
                not_before: format_timestamp(not_before),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AuthorizationDecisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = AuthorizationDecisionWire::deserialize(deserializer)?;
        match wire {
            AuthorizationDecisionWire::Allow { grant } => Ok(Self::Allow { grant }),
            AuthorizationDecisionWire::AllowWithConstraints { grant } => {
                Ok(Self::AllowWithConstraints { grant })
            }
            AuthorizationDecisionWire::Deny => Ok(Self::Deny),
            AuthorizationDecisionWire::RequireReview { review_id } => {
                validate_nonempty_string("authorization.review_id", &review_id)
                    .map_err(de::Error::custom)?;
                Ok(Self::RequireReview { review_id })
            }
            AuthorizationDecisionWire::RetryLater { not_before } => Ok(Self::RetryLater {
                not_before: parse_exact_timestamp(&not_before).map_err(de::Error::custom)?,
            }),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
enum AuthorizationDecisionWireRef<'a> {
    Allow { grant: &'a CapabilitySetV1 },
    AllowWithConstraints { grant: &'a CapabilitySetV1 },
    Deny,
    RequireReview { review_id: &'a str },
    RetryLater { not_before: String },
}

#[derive(Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
enum AuthorizationDecisionWire {
    Allow { grant: CapabilitySetV1 },
    AllowWithConstraints { grant: CapabilitySetV1 },
    Deny,
    RequireReview { review_id: String },
    RetryLater { not_before: String },
}

/// Signature algorithm for authorization proofs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureAlgorithmV1 {
    /// Strict Ed25519 signatures.
    Ed25519,
}

/// Unsigned, canonical authorization statement.
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl AuthorizationStatementV1 {
    /// Creates a validated unsigned authorization statement.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_identity: ExecutionIdentityV1,
        intent_digest: ContentDigestV1,
        authority: ActorIdentityV1,
        policy: PolicyIdentityV1,
        decision: AuthorizationDecisionV1,
        mut reasons: Vec<DecisionReasonV1>,
        algorithm: SignatureAlgorithmV1,
        key_id: impl Into<String>,
        audience: impl Into<String>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        nonce: AuthorizationNonce,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let key_id = key_id.into();
        let audience = audience.into();
        validate_nonempty_string("authorization.key_id", &key_id)?;
        validate_nonempty_string("authorization.audience", &audience)?;
        policy.validate()?;
        decision.validate()?;
        for reason in &reasons {
            reason.validate()?;
        }
        if reasons.len() > MAX_DECISION_REASONS {
            return Err(invalid_authorization("more than 32 decision reasons"));
        }
        reasons.sort();
        if reasons.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid_authorization("duplicate decision reason"));
        }
        if issued_at >= expires_at {
            return Err(invalid_authorization(
                "expires_at must be later than issued_at",
            ));
        }
        Ok(Self {
            schema_version: EXECUTION_ENVELOPE_SCHEMA_V1,
            execution_identity,
            intent_digest,
            authority,
            policy,
            decision,
            reasons,
            algorithm,
            key_id,
            audience,
            issued_at,
            expires_at,
            nonce,
        })
    }

    /// Returns domain-separated canonical bytes signed by the authority.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, ExecutionEnvelopeError> {
        let raw =
            serde_json::to_vec(self).map_err(|error| ExecutionEnvelopeError::Canonicalization {
                reason: error.to_string(),
            })?;
        let canonical = canonicalize_json_v1(&raw)?;
        let mut framed = b"crux.execution.authorization.v1\0".to_vec();
        framed.extend_from_slice(&canonical);
        Ok(framed)
    }

    /// Computes the SHA-256 digest of the signing bytes.
    pub fn payload_digest(&self) -> Result<ContentDigestV1, ExecutionEnvelopeError> {
        Ok(ContentDigestV1::digest(&self.signing_bytes()?))
    }

    /// Borrows the bound execution identity.
    #[must_use]
    pub const fn execution_identity(&self) -> &ExecutionIdentityV1 {
        &self.execution_identity
    }

    /// Borrows the bound intent record digest.
    #[must_use]
    pub const fn intent_digest(&self) -> &ContentDigestV1 {
        &self.intent_digest
    }

    /// Borrows the authorizing actor.
    #[must_use]
    pub const fn authority(&self) -> &ActorIdentityV1 {
        &self.authority
    }

    /// Borrows the authorizing policy identity.
    #[must_use]
    pub const fn policy(&self) -> &PolicyIdentityV1 {
        &self.policy
    }

    /// Borrows the policy decision.
    #[must_use]
    pub const fn decision(&self) -> &AuthorizationDecisionV1 {
        &self.decision
    }

    /// Borrows coded decision reasons.
    #[must_use]
    pub fn reasons(&self) -> &[DecisionReasonV1] {
        &self.reasons
    }

    /// Borrows the trusted signing key identifier.
    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Borrows the authorization audience.
    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }

    /// Returns the signature algorithm named by the statement.
    #[must_use]
    pub const fn algorithm(&self) -> SignatureAlgorithmV1 {
        self.algorithm
    }

    /// Borrows the instant at which the authorization was issued.
    #[must_use]
    pub const fn issued_at(&self) -> &DateTime<Utc> {
        &self.issued_at
    }

    /// Borrows the instant at which the authorization expires.
    #[must_use]
    pub const fn expires_at(&self) -> &DateTime<Utc> {
        &self.expires_at
    }

    /// Borrows the single-use authorization nonce.
    #[must_use]
    pub const fn nonce(&self) -> &AuthorizationNonce {
        &self.nonce
    }
}

impl Serialize for AuthorizationStatementV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        AuthorizationStatementWireRef {
            schema_version: self.schema_version,
            execution_identity: &self.execution_identity,
            intent_digest: &self.intent_digest,
            authority: &self.authority,
            policy: &self.policy,
            decision: &self.decision,
            reasons: &self.reasons,
            algorithm: self.algorithm,
            key_id: &self.key_id,
            audience: &self.audience,
            issued_at: format_timestamp(&self.issued_at),
            expires_at: format_timestamp(&self.expires_at),
            nonce: &self.nonce,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AuthorizationStatementV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = AuthorizationStatementWire::deserialize(deserializer)?;
        if wire.schema_version != EXECUTION_ENVELOPE_SCHEMA_V1 {
            return Err(de::Error::custom(
                "unsupported authorization schema version",
            ));
        }
        let issued_at = parse_exact_timestamp(&wire.issued_at).map_err(de::Error::custom)?;
        let expires_at = parse_exact_timestamp(&wire.expires_at).map_err(de::Error::custom)?;
        Self::new(
            wire.execution_identity,
            wire.intent_digest,
            wire.authority,
            wire.policy,
            wire.decision,
            wire.reasons,
            wire.algorithm,
            wire.key_id,
            wire.audience,
            issued_at,
            expires_at,
            wire.nonce,
        )
        .map_err(de::Error::custom)
    }
}

#[derive(Serialize)]
struct AuthorizationStatementWireRef<'a> {
    schema_version: u32,
    execution_identity: &'a ExecutionIdentityV1,
    intent_digest: &'a ContentDigestV1,
    authority: &'a ActorIdentityV1,
    policy: &'a PolicyIdentityV1,
    decision: &'a AuthorizationDecisionV1,
    reasons: &'a [DecisionReasonV1],
    algorithm: SignatureAlgorithmV1,
    key_id: &'a str,
    audience: &'a str,
    issued_at: String,
    expires_at: String,
    nonce: &'a AuthorizationNonce,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizationStatementWire {
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
    issued_at: String,
    expires_at: String,
    nonce: AuthorizationNonce,
}

/// Digest and Ed25519 signature over an authorization statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorizationProofV1 {
    payload_digest: ContentDigestV1,
    signature: String,
}

impl AuthorizationProofV1 {
    /// Creates a well-formed Ed25519 proof.
    pub fn new(
        payload_digest: ContentDigestV1,
        signature: impl Into<String>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let signature = signature.into();
        let decoded = URL_SAFE_NO_PAD
            .decode(signature.as_bytes())
            .map_err(|_| invalid_authorization("signature must be unpadded base64url"))?;
        if decoded.len() != 64 || signature.contains('=') {
            return Err(invalid_authorization(
                "Ed25519 signature must contain exactly 64 bytes",
            ));
        }
        Ok(Self {
            payload_digest,
            signature,
        })
    }

    /// Borrows the signed payload digest.
    #[must_use]
    pub const fn payload_digest(&self) -> &ContentDigestV1 {
        &self.payload_digest
    }

    /// Borrows the unpadded base64url signature.
    #[must_use]
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Decodes the signature into its transport-neutral 64-byte representation.
    pub fn signature_bytes(&self) -> Result<[u8; 64], ExecutionEnvelopeError> {
        URL_SAFE_NO_PAD
            .decode(self.signature.as_bytes())
            .map_err(|_| invalid_authorization("signature must be unpadded base64url"))?
            .try_into()
            .map_err(|_| invalid_authorization("Ed25519 signature must contain exactly 64 bytes"))
    }
}

impl<'de> Deserialize<'de> for AuthorizationProofV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ProofWire {
            payload_digest: ContentDigestV1,
            signature: String,
        }

        let wire = ProofWire::deserialize(deserializer)?;
        Self::new(wire.payload_digest, wire.signature).map_err(de::Error::custom)
    }
}

/// Complete externally signed authorization record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorizationRecordV1 {
    statement: AuthorizationStatementV1,
    proof: AuthorizationProofV1,
}

impl AuthorizationRecordV1 {
    /// Creates a record whose proof digest matches its statement.
    pub fn new(
        statement: AuthorizationStatementV1,
        proof: AuthorizationProofV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        if statement.payload_digest()? != *proof.payload_digest() {
            return Err(invalid_authorization(
                "proof payload digest does not match statement",
            ));
        }
        Ok(Self { statement, proof })
    }

    /// Borrows the signed statement.
    #[must_use]
    pub const fn statement(&self) -> &AuthorizationStatementV1 {
        &self.statement
    }

    /// Borrows the signature proof.
    #[must_use]
    pub const fn proof(&self) -> &AuthorizationProofV1 {
        &self.proof
    }
}

impl<'de> Deserialize<'de> for AuthorizationRecordV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct AuthorizationWire {
            statement: AuthorizationStatementV1,
            proof: AuthorizationProofV1,
        }

        let wire = AuthorizationWire::deserialize(deserializer)?;
        Self::new(wire.statement, wire.proof).map_err(de::Error::custom)
    }
}

/// Versioned identity for an executor, runtime, tool, or verifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ComponentIdentityV1 {
    /// Component name.
    pub name: String,
    /// Component version.
    pub version: String,
    /// Optional immutable binary or package digest.
    pub digest: Option<ContentDigestV1>,
}

impl ComponentIdentityV1 {
    /// Creates a validated component identity.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        digest: Option<ContentDigestV1>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let name = name.into();
        let version = version.into();
        validate_nonempty_string("component.name", &name)?;
        validate_nonempty_string("component.version", &version)?;
        Ok(Self {
            name,
            version,
            digest,
        })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(self.name.clone(), self.version.clone(), self.digest.clone()).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for ComponentIdentityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ComponentWire {
            name: String,
            version: String,
            digest: Option<ContentDigestV1>,
        }
        let wire = ComponentWire::deserialize(deserializer)?;
        Self::new(wire.name, wire.version, wire.digest).map_err(de::Error::custom)
    }
}

/// Immutable image identity resolved by an executor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ImageIdentityV1 {
    /// Human-readable image reference.
    pub reference: String,
    /// Immutable image manifest digest.
    pub digest: ContentDigestV1,
}

impl ImageIdentityV1 {
    /// Creates a validated immutable image identity.
    pub fn new(
        reference: impl Into<String>,
        digest: ContentDigestV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let reference = reference.into();
        validate_nonempty_string("image.reference", &reference)?;
        Ok(Self { reference, digest })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(self.reference.clone(), self.digest.clone()).map(|_| ())
    }
}

impl<'de> Deserialize<'de> for ImageIdentityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ImageWire {
            reference: String,
            digest: ContentDigestV1,
        }
        let wire = ImageWire::deserialize(deserializer)?;
        Self::new(wire.reference, wire.digest).map_err(de::Error::custom)
    }
}

/// Digest-addressed evidence stored outside the envelope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct EvidenceRefV1 {
    /// Evidence media type.
    pub media_type: String,
    /// Digest of the complete evidence bytes.
    pub digest: ContentDigestV1,
    /// Complete evidence size in bytes.
    pub size_bytes: u64,
    /// Optional executor-defined locator.
    pub locator: Option<String>,
}

impl EvidenceRefV1 {
    /// Creates a validated evidence reference.
    pub fn new(
        media_type: impl Into<String>,
        digest: ContentDigestV1,
        size_bytes: u64,
        locator: Option<String>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let media_type = media_type.into();
        validate_nonempty_string("evidence.media_type", &media_type)?;
        validate_safe_integer("evidence.size_bytes", size_bytes)?;
        if let Some(locator) = &locator {
            validate_nonempty_string("evidence.locator", locator)?;
        }
        Ok(Self {
            media_type,
            digest,
            size_bytes,
            locator,
        })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(
            self.media_type.clone(),
            self.digest.clone(),
            self.size_bytes,
            self.locator.clone(),
        )
        .map(|_| ())
    }
}

impl<'de> Deserialize<'de> for EvidenceRefV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EvidenceWire {
            media_type: String,
            digest: ContentDigestV1,
            size_bytes: u64,
            locator: Option<String>,
        }
        let wire = EvidenceWire::deserialize(deserializer)?;
        Self::new(wire.media_type, wire.digest, wire.size_bytes, wire.locator)
            .map_err(de::Error::custom)
    }
}

/// Executor-local admission disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionDecisionV1 {
    /// Workload may proceed under the effective capability set.
    Admitted,
    /// Workload is denied before resource allocation.
    Denied,
}

/// Executor-local admission evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdmissionRecordV1 {
    /// Executor making the admission decision.
    pub executor: ComponentIdentityV1,
    /// Local policy that made the decision.
    pub policy: PolicyIdentityV1,
    /// Admission disposition.
    pub decision: AdmissionDecisionV1,
    /// Effective capabilities for admitted work.
    pub effective_capabilities: Option<CapabilitySetV1>,
    /// Coded admission reasons.
    pub reasons: Vec<DecisionReasonV1>,
}

impl AdmissionRecordV1 {
    /// Creates an admitted record with required effective capabilities.
    pub fn admitted(
        executor: ComponentIdentityV1,
        policy: PolicyIdentityV1,
        effective_capabilities: CapabilitySetV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        effective_capabilities.validate()?;
        Ok(Self {
            executor,
            policy,
            decision: AdmissionDecisionV1::Admitted,
            effective_capabilities: Some(effective_capabilities),
            reasons: Vec::new(),
        })
    }

    /// Creates a terminal denied admission record.
    pub fn denied(
        executor: ComponentIdentityV1,
        policy: PolicyIdentityV1,
        mut reasons: Vec<DecisionReasonV1>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        if reasons.is_empty() || reasons.len() > 32 {
            return Err(invalid_capability(
                "admission.reasons",
                "denial requires 1 to 32 reasons",
            ));
        }
        for reason in &reasons {
            reason.validate()?;
        }
        reasons.sort();
        if reasons.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid_capability(
                "admission.reasons",
                "duplicate denial reason",
            ));
        }
        Ok(Self {
            executor,
            policy,
            decision: AdmissionDecisionV1::Denied,
            effective_capabilities: None,
            reasons,
        })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        self.executor.validate()?;
        self.policy.validate()?;
        for reason in &self.reasons {
            reason.validate()?;
        }
        if self.reasons.len() > MAX_DECISION_REASONS {
            return Err(invalid_capability(
                "admission.reasons",
                "more than 32 reasons",
            ));
        }
        if self.reasons.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invalid_capability(
                "admission.reasons",
                "reasons must be unique and canonically sorted",
            ));
        }
        match (&self.decision, &self.effective_capabilities) {
            (AdmissionDecisionV1::Admitted, Some(capabilities)) if self.reasons.is_empty() => {
                capabilities.validate()
            }
            (AdmissionDecisionV1::Denied, None) if !self.reasons.is_empty() => Ok(()),
            _ => Err(invalid_capability(
                "admission.effective_capabilities",
                "admitted requires capabilities and no reasons; denied requires reasons and forbids capabilities",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for AdmissionRecordV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct AdmissionWire {
            executor: ComponentIdentityV1,
            policy: PolicyIdentityV1,
            decision: AdmissionDecisionV1,
            effective_capabilities: Option<CapabilitySetV1>,
            reasons: Vec<DecisionReasonV1>,
        }
        let wire = AdmissionWire::deserialize(deserializer)?;
        let record = Self {
            executor: wire.executor,
            policy: wire.policy,
            decision: wire.decision,
            effective_capabilities: wire.effective_capabilities,
            reasons: wire.reasons,
        };
        record.validate().map_err(de::Error::custom)?;
        Ok(record)
    }
}

/// Effective environment selected by an executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionEnvironmentV1 {
    /// Executor identity.
    pub executor: ComponentIdentityV1,
    /// Concrete runtime identity.
    pub runtime: ComponentIdentityV1,
    /// Optional immutable image identity.
    pub image: Option<ImageIdentityV1>,
    /// Effective enforced capabilities.
    pub effective_capabilities: CapabilitySetV1,
    /// References to native manifests or attestations.
    pub native_evidence: Vec<EvidenceRefV1>,
}

impl<'de> Deserialize<'de> for ExecutionEnvironmentV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EnvironmentWire {
            executor: ComponentIdentityV1,
            runtime: ComponentIdentityV1,
            image: Option<ImageIdentityV1>,
            effective_capabilities: CapabilitySetV1,
            native_evidence: Vec<EvidenceRefV1>,
        }
        let wire = EnvironmentWire::deserialize(deserializer)?;
        Self::new(
            wire.executor,
            wire.runtime,
            wire.image,
            wire.effective_capabilities,
            wire.native_evidence,
        )
        .map_err(de::Error::custom)
    }
}

impl ExecutionEnvironmentV1 {
    /// Creates a validated execution environment.
    pub fn new(
        executor: ComponentIdentityV1,
        runtime: ComponentIdentityV1,
        image: Option<ImageIdentityV1>,
        effective_capabilities: CapabilitySetV1,
        native_evidence: Vec<EvidenceRefV1>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let mut native_evidence = native_evidence;
        if native_evidence.len() > MAX_EVIDENCE_REFS {
            return Err(invalid_capability(
                "environment.native_evidence",
                "more than 64 evidence references",
            ));
        }
        native_evidence
            .sort_by(|left, right| evidence_sort_key(left).cmp(&evidence_sort_key(right)));
        if native_evidence.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid_capability(
                "environment.native_evidence",
                "duplicate evidence reference",
            ));
        }
        let environment = Self {
            executor,
            runtime,
            image,
            effective_capabilities,
            native_evidence,
        };
        environment.validate()?;
        Ok(environment)
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        self.executor.validate()?;
        self.runtime.validate()?;
        if let Some(image) = &self.image {
            image.validate()?;
        }
        self.effective_capabilities.validate()?;
        if self.native_evidence.len() > MAX_EVIDENCE_REFS {
            return Err(invalid_capability(
                "environment.native_evidence",
                "more than 64 evidence references",
            ));
        }
        for evidence in &self.native_evidence {
            evidence.validate()?;
        }
        if self
            .native_evidence
            .windows(2)
            .any(|pair| evidence_sort_key(&pair[0]) >= evidence_sort_key(&pair[1]))
        {
            return Err(invalid_capability(
                "environment.native_evidence",
                "evidence must be unique and canonically sorted",
            ));
        }
        Ok(())
    }
}

/// Durable release-authorization instant after which external work may begin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionStartedV1 {
    /// Instant at which the durable record authorizes release of external work.
    pub started_at: DateTime<Utc>,
}

impl ExecutionStartedV1 {
    /// Creates a start record payload.
    #[must_use]
    pub const fn new(started_at: DateTime<Utc>) -> Self {
        Self { started_at }
    }
}

impl Serialize for ExecutionStartedV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        StartedWire {
            started_at: format_timestamp(&self.started_at),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExecutionStartedV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = StartedWire::deserialize(deserializer)?;
        Ok(Self::new(
            parse_exact_timestamp(&wire.started_at).map_err(de::Error::custom)?,
        ))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartedWire {
    started_at: String,
}

/// Output channel represented by digest-only evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStreamV1 {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// Structured tool result.
    Structured,
}

/// Bounded output evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionOutputV1 {
    /// Output channel.
    pub stream: OutputStreamV1,
    /// Digest-addressed complete output evidence.
    pub content: EvidenceRefV1,
    /// Complete byte count observed by the executor.
    pub observed_bytes: u64,
    /// Bytes forwarded to the caller.
    pub forwarded_bytes: u64,
    /// Whether caller-visible bytes were truncated.
    pub truncated: bool,
}

impl ExecutionOutputV1 {
    /// Creates validated digest-only output evidence.
    pub fn new(
        stream: OutputStreamV1,
        content: EvidenceRefV1,
        observed_bytes: u64,
        forwarded_bytes: u64,
        truncated: bool,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let output = Self {
            stream,
            content,
            observed_bytes,
            forwarded_bytes,
            truncated,
        };
        output.validate()?;
        Ok(output)
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        self.content.validate()?;
        validate_safe_integer("output.observed_bytes", self.observed_bytes)?;
        validate_safe_integer("output.forwarded_bytes", self.forwarded_bytes)?;
        if self.forwarded_bytes > self.observed_bytes
            || self.content.size_bytes != self.observed_bytes
            || self.truncated != (self.forwarded_bytes < self.observed_bytes)
        {
            return Err(invalid_capability(
                "output",
                "byte counts, evidence size, and truncation flag are inconsistent",
            ));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ExecutionOutputV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct OutputWire {
            stream: OutputStreamV1,
            content: EvidenceRefV1,
            observed_bytes: u64,
            forwarded_bytes: u64,
            truncated: bool,
        }
        let wire = OutputWire::deserialize(deserializer)?;
        Self::new(
            wire.stream,
            wire.content,
            wire.observed_bytes,
            wire.forwarded_bytes,
            wire.truncated,
        )
        .map_err(de::Error::custom)
    }
}

/// Named artifact produced by an execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactRefV1 {
    /// Stable artifact name.
    pub name: String,
    /// Digest-addressed artifact evidence.
    pub evidence: EvidenceRefV1,
}

impl ArtifactRefV1 {
    /// Creates a validated named artifact reference.
    pub fn new(
        name: impl Into<String>,
        evidence: EvidenceRefV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let name = name.into();
        validate_nonempty_string("artifact.name", &name)?;
        Ok(Self { name, evidence })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        validate_nonempty_string("artifact.name", &self.name)?;
        self.evidence.validate()
    }
}

impl<'de> Deserialize<'de> for ArtifactRefV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ArtifactWire {
            name: String,
            evidence: EvidenceRefV1,
        }
        let wire = ArtifactWire::deserialize(deserializer)?;
        Self::new(wire.name, wire.evidence).map_err(de::Error::custom)
    }
}

/// Terminal workload status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatusV1 {
    /// Workload completed successfully.
    Succeeded,
    /// Workload or preparation failed.
    Failed,
    /// Workload was cancelled.
    Cancelled,
    /// Workload exceeded its wall-time budget.
    TimedOut,
    /// Execution was denied before start.
    Denied,
}

/// Phase in which execution failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFailurePhaseV1 {
    /// Authorization or local admission.
    Admission,
    /// Resource and rootfs preparation.
    Preparation,
    /// Staged process spawn.
    Spawn,
    /// Running workload.
    Runtime,
    /// Resource cleanup.
    Cleanup,
}

/// Stable classified execution error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionErrorV1 {
    /// Failure phase.
    pub phase: ExecutionFailurePhaseV1,
    /// Stable executor-defined classification.
    pub classification: String,
    /// Human-readable error message.
    pub message: String,
}

impl ExecutionErrorV1 {
    /// Creates a validated classified error.
    pub fn new(
        phase: ExecutionFailurePhaseV1,
        classification: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let classification = classification.into();
        let message = message.into();
        validate_nonempty_string("error.classification", &classification)?;
        validate_nonempty_string("error.message", &message)?;
        Ok(Self {
            phase,
            classification,
            message,
        })
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        Self::new(
            self.phase,
            self.classification.clone(),
            self.message.clone(),
        )
        .map(|_| ())
    }
}

impl<'de> Deserialize<'de> for ExecutionErrorV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ErrorWire {
            phase: ExecutionFailurePhaseV1,
            classification: String,
            message: String,
        }
        let wire = ErrorWire::deserialize(deserializer)?;
        Self::new(wire.phase, wire.classification, wire.message).map_err(de::Error::custom)
    }
}

/// Replay eligibility and causal source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayMetadataV1 {
    /// Whether an executor may replay the result without repeating side effects.
    pub replayable: bool,
    /// Source execution when this result came from replay.
    pub source_execution_id: Option<ExecutionId>,
}

impl ReplayMetadataV1 {
    /// Creates replay metadata.
    #[must_use]
    pub const fn new(replayable: bool, source_execution_id: Option<ExecutionId>) -> Self {
        Self {
            replayable,
            source_execution_id,
        }
    }
}

/// Terminal execution result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResultV1 {
    /// Terminal status.
    pub status: ExecutionStatusV1,
    /// Completion instant.
    pub completed_at: DateTime<Utc>,
    /// Process exit code when available.
    pub exit_code: Option<i32>,
    /// Optional bounded normalized observation.
    pub observation: Option<serde_json::Value>,
    /// Classified failure details.
    pub error: Option<ExecutionErrorV1>,
    /// Replay metadata.
    pub replay: ReplayMetadataV1,
}

impl ExecutionResultV1 {
    /// Creates a terminal result payload.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status: ExecutionStatusV1,
        completed_at: DateTime<Utc>,
        exit_code: Option<i32>,
        observation: Option<serde_json::Value>,
        error: Option<ExecutionErrorV1>,
        replay: ReplayMetadataV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let result = Self {
            status,
            completed_at,
            exit_code,
            observation,
            error,
            replay,
        };
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        if let Some(observation) = &self.observation {
            validate_json(observation)?;
        }
        if let Some(error) = &self.error {
            error.validate()?;
        }
        if matches!(
            self.status,
            ExecutionStatusV1::Failed | ExecutionStatusV1::Cancelled | ExecutionStatusV1::TimedOut
        ) && self.error.is_none()
        {
            return Err(ExecutionEnvelopeError::InvalidTerminalState {
                reason: "non-success result requires a classified error".to_string(),
            });
        }
        if self.status == ExecutionStatusV1::Succeeded && self.error.is_some() {
            return Err(ExecutionEnvelopeError::InvalidTerminalState {
                reason: "successful result forbids an error".to_string(),
            });
        }
        Ok(())
    }
}

impl Serialize for ExecutionResultV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ResultWireRef {
            status: self.status,
            completed_at: format_timestamp(&self.completed_at),
            exit_code: self.exit_code,
            observation: self.observation.as_ref(),
            error: self.error.as_ref(),
            replay: &self.replay,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExecutionResultV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ResultWire::deserialize(deserializer)?;
        Self::new(
            wire.status,
            parse_exact_timestamp(&wire.completed_at).map_err(de::Error::custom)?,
            wire.exit_code,
            wire.observation.map(|value| value.0),
            wire.error,
            wire.replay,
        )
        .map_err(de::Error::custom)
    }
}

#[derive(Serialize)]
struct ResultWireRef<'a> {
    status: ExecutionStatusV1,
    completed_at: String,
    exit_code: Option<i32>,
    observation: Option<&'a serde_json::Value>,
    error: Option<&'a ExecutionErrorV1>,
    replay: &'a ReplayMetadataV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultWire {
    status: ExecutionStatusV1,
    completed_at: String,
    exit_code: Option<i32>,
    observation: Option<UniqueJsonValue>,
    error: Option<ExecutionErrorV1>,
    replay: ReplayMetadataV1,
}

/// Independent verification disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatusV1 {
    /// All named checks passed.
    Passed,
    /// At least one named check failed.
    Failed,
    /// Evidence was insufficient for a decision.
    Inconclusive,
}

/// Verification attached to an earlier terminal record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationRecordV1 {
    /// Verifier identity.
    pub verifier: ComponentIdentityV1,
    /// Terminal record being verified.
    pub subject_record_id: ExecutionRecordId,
    /// Digest of the terminal record being verified.
    pub subject_digest: ContentDigestV1,
    /// Verification disposition.
    pub status: VerificationStatusV1,
    /// Stable check names.
    pub checks: Vec<String>,
    /// Digest-addressed verification evidence.
    pub evidence: Vec<EvidenceRefV1>,
}

impl VerificationRecordV1 {
    /// Creates a validated verification record.
    pub fn new(
        verifier: ComponentIdentityV1,
        subject_record_id: ExecutionRecordId,
        subject_digest: ContentDigestV1,
        status: VerificationStatusV1,
        mut checks: Vec<String>,
        mut evidence: Vec<EvidenceRefV1>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        if evidence.len() > MAX_EVIDENCE_REFS {
            return Err(invalid_capability(
                "verification.evidence",
                "more than 64 evidence references",
            ));
        }
        for check in &checks {
            validate_nonempty_string("verification.checks", check)?;
        }
        sort_unique_strings("verification.checks", &mut checks)?;
        evidence.sort_by(|left, right| evidence_sort_key(left).cmp(&evidence_sort_key(right)));
        if evidence.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid_capability(
                "verification.evidence",
                "duplicate evidence reference",
            ));
        }
        let verification = Self {
            verifier,
            subject_record_id,
            subject_digest,
            status,
            checks,
            evidence,
        };
        verification.validate()?;
        Ok(verification)
    }

    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        self.verifier.validate()?;
        if self.evidence.len() > MAX_EVIDENCE_REFS {
            return Err(invalid_capability(
                "verification.evidence",
                "more than 64 evidence references",
            ));
        }
        for check in &self.checks {
            validate_nonempty_string("verification.checks", check)?;
        }
        if self.checks.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invalid_capability(
                "verification.checks",
                "checks must be unique and canonically sorted",
            ));
        }
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        if self
            .evidence
            .windows(2)
            .any(|pair| evidence_sort_key(&pair[0]) >= evidence_sort_key(&pair[1]))
        {
            return Err(invalid_capability(
                "verification.evidence",
                "evidence must be unique and canonically sorted",
            ));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for VerificationRecordV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct VerificationWire {
            verifier: ComponentIdentityV1,
            subject_record_id: ExecutionRecordId,
            subject_digest: ContentDigestV1,
            status: VerificationStatusV1,
            checks: Vec<String>,
            evidence: Vec<EvidenceRefV1>,
        }
        let wire = VerificationWire::deserialize(deserializer)?;
        Self::new(
            wire.verifier,
            wire.subject_record_id,
            wire.subject_digest,
            wire.status,
            wire.checks,
            wire.evidence,
        )
        .map_err(de::Error::custom)
    }
}

/// Payload carried by one hash-linked execution record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ExecutionRecordPayloadV1 {
    /// Initial execution intent.
    Intent(Box<ExecutionIntentV1>),
    /// Externally signed authorization decision.
    Authorization(Box<AuthorizationRecordV1>),
    /// Executor-local admission decision.
    Admission(Box<AdmissionRecordV1>),
    /// Effective execution environment.
    Environment(Box<ExecutionEnvironmentV1>),
    /// Workload release instant.
    Started(ExecutionStartedV1),
    /// Digest-addressed output evidence.
    Output(Box<ExecutionOutputV1>),
    /// Named execution artifact.
    Artifact(Box<ArtifactRefV1>),
    /// Terminal execution result.
    Result(Box<ExecutionResultV1>),
    /// Independent terminal verification.
    Verification(Box<VerificationRecordV1>),
}

impl ExecutionRecordPayloadV1 {
    fn validate(&self) -> Result<(), ExecutionEnvelopeError> {
        match self {
            Self::Intent(intent) => intent.validate(),
            Self::Authorization(authorization) => AuthorizationRecordV1::new(
                authorization.statement.clone(),
                authorization.proof.clone(),
            )
            .map(|_| ()),
            Self::Admission(admission) => admission.validate(),
            Self::Environment(environment) => environment.validate(),
            Self::Started(_) => Ok(()),
            Self::Output(output) => output.validate(),
            Self::Artifact(artifact) => artifact.validate(),
            Self::Result(result) => result.validate(),
            Self::Verification(verification) => verification.validate(),
        }
    }
}

/// One immutable record in a governed execution chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRecordV1 {
    record_id: ExecutionRecordId,
    sequence: u64,
    recorded_at: DateTime<Utc>,
    previous_digest: Option<ContentDigestV1>,
    digest: ContentDigestV1,
    payload: ExecutionRecordPayloadV1,
}

impl ExecutionRecordV1 {
    /// Creates a record and computes its canonical digest.
    pub fn new(
        record_id: ExecutionRecordId,
        sequence: u64,
        recorded_at: DateTime<Utc>,
        previous_digest: Option<ContentDigestV1>,
        payload: ExecutionRecordPayloadV1,
    ) -> Result<Self, ExecutionEnvelopeError> {
        validate_safe_integer("record.sequence", sequence)?;
        payload.validate()?;
        let digest = record_digest(
            &record_id,
            sequence,
            &recorded_at,
            previous_digest.as_ref(),
            &payload,
        )?;
        Ok(Self {
            record_id,
            sequence,
            recorded_at,
            previous_digest,
            digest,
            payload,
        })
    }

    /// Borrows the record identifier.
    #[must_use]
    pub const fn record_id(&self) -> &ExecutionRecordId {
        &self.record_id
    }

    /// Returns the zero-based record sequence.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Borrows the normalized UTC timestamp.
    #[must_use]
    pub const fn recorded_at(&self) -> &DateTime<Utc> {
        &self.recorded_at
    }

    /// Borrows the predecessor digest when this is not the first record.
    #[must_use]
    pub const fn previous_digest(&self) -> Option<&ContentDigestV1> {
        self.previous_digest.as_ref()
    }

    /// Borrows the canonical record digest.
    #[must_use]
    pub const fn digest(&self) -> &ContentDigestV1 {
        &self.digest
    }

    /// Borrows the typed record payload.
    #[must_use]
    pub const fn payload(&self) -> &ExecutionRecordPayloadV1 {
        &self.payload
    }

    fn verify_digest(&self) -> Result<(), ExecutionEnvelopeError> {
        self.payload.validate()?;
        let expected = record_digest(
            &self.record_id,
            self.sequence,
            &self.recorded_at,
            self.previous_digest.as_ref(),
            &self.payload,
        )?;
        if expected != self.digest {
            return Err(ExecutionEnvelopeError::InvalidDigest {
                sequence: self.sequence,
            });
        }
        Ok(())
    }
}

impl Serialize for ExecutionRecordV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RecordWireRef {
            record_id: &self.record_id,
            sequence: self.sequence,
            recorded_at: format_timestamp(&self.recorded_at),
            previous_digest: self.previous_digest.as_ref(),
            digest: &self.digest,
            payload: &self.payload,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExecutionRecordV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RecordWire::deserialize(deserializer)?;
        let recorded_at = DateTime::parse_from_rfc3339(&wire.recorded_at)
            .map_err(de::Error::custom)?
            .with_timezone(&Utc);
        if format_timestamp(&recorded_at) != wire.recorded_at {
            return Err(de::Error::custom(
                "recorded_at must use UTC and nine fractional digits",
            ));
        }
        let record = Self {
            record_id: wire.record_id,
            sequence: wire.sequence,
            recorded_at,
            previous_digest: wire.previous_digest,
            digest: wire.digest,
            payload: wire.payload,
        };
        record.verify_digest().map_err(de::Error::custom)?;
        Ok(record)
    }
}

#[derive(Serialize)]
struct RecordWireRef<'a> {
    record_id: &'a ExecutionRecordId,
    sequence: u64,
    recorded_at: String,
    previous_digest: Option<&'a ContentDigestV1>,
    digest: &'a ContentDigestV1,
    payload: &'a ExecutionRecordPayloadV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordWire {
    record_id: ExecutionRecordId,
    sequence: u64,
    recorded_at: String,
    previous_digest: Option<ContentDigestV1>,
    digest: ContentDigestV1,
    payload: ExecutionRecordPayloadV1,
}

#[derive(Serialize)]
struct RecordDigestProjection<'a> {
    record_id: &'a ExecutionRecordId,
    sequence: u64,
    recorded_at: String,
    previous_digest: Option<&'a ContentDigestV1>,
    payload: &'a ExecutionRecordPayloadV1,
}

/// Validated lifecycle state of an execution envelope prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLifecycleV1 {
    /// Intent exists without an authorization decision.
    Proposed,
    /// External authority denied execution.
    AuthorizationDenied,
    /// External authority requires review.
    ReviewRequired,
    /// External authority deferred execution.
    RetryLater,
    /// External authority allowed execution.
    Authorized,
    /// Executor denied local admission.
    AdmissionDenied,
    /// Executor admitted the workload.
    Admitted,
    /// Effective environment has been persisted.
    Prepared,
    /// Workload code has been released.
    Running,
    /// A terminal result has been persisted.
    Completed,
    /// At least one verification follows terminal evidence.
    Verified,
}

/// Complete hash-linked record collection for one execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionEnvelopeV1 {
    schema_version: u32,
    identity: ExecutionIdentityV1,
    records: Vec<ExecutionRecordV1>,
}

impl ExecutionEnvelopeV1 {
    /// Creates an intent-only envelope.
    pub fn new(
        identity: ExecutionIdentityV1,
        intent: ExecutionIntentV1,
        recorded_at: DateTime<Utc>,
    ) -> Result<Self, ExecutionEnvelopeError> {
        let first = ExecutionRecordV1::new(
            ExecutionRecordId::new(),
            0,
            recorded_at,
            None,
            ExecutionRecordPayloadV1::Intent(Box::new(intent)),
        )?;
        let envelope = Self {
            schema_version: EXECUTION_ENVELOPE_SCHEMA_V1,
            identity,
            records: vec![first],
        };
        envelope.validate_prefix()?;
        Ok(envelope)
    }

    /// Parses untrusted JSON bytes after duplicate-key, depth, size, and number admission.
    pub fn from_json_bytes(input: &[u8]) -> Result<Self, ExecutionEnvelopeError> {
        let canonical = canonicalize_json_v1(input)?;
        serde_json::from_slice(&canonical).map_err(|error| {
            ExecutionEnvelopeError::Canonicalization {
                reason: error.to_string(),
            }
        })
    }

    /// Appends one record while preserving all prefix invariants.
    pub fn append(
        &mut self,
        payload: ExecutionRecordPayloadV1,
        recorded_at: DateTime<Utc>,
    ) -> Result<&ExecutionRecordV1, ExecutionEnvelopeError> {
        let sequence = u64::try_from(self.records.len()).map_err(|_| {
            ExecutionEnvelopeError::InvalidRecordOrder {
                sequence: u64::MAX,
                reason: "record count overflow".to_string(),
            }
        })?;
        let previous_digest = self.records.last().map(|record| record.digest().clone());
        let record = ExecutionRecordV1::new(
            ExecutionRecordId::new(),
            sequence,
            recorded_at,
            previous_digest,
            payload,
        )?;
        self.records.push(record);
        if let Err(error) = self.validate_prefix() {
            self.records.pop();
            return Err(error);
        }
        self.records
            .last()
            .ok_or_else(|| ExecutionEnvelopeError::InvalidRecordOrder {
                sequence,
                reason: "appended record missing".to_string(),
            })
    }

    /// Validates record integrity and any valid durable lifecycle prefix.
    pub fn validate_prefix(&self) -> Result<(), ExecutionEnvelopeError> {
        self.analyze().map(|_| ())
    }

    /// Validates a terminal envelope.
    pub fn validate_complete(&self) -> Result<(), ExecutionEnvelopeError> {
        let analysis = self.analyze()?;
        if !analysis.complete {
            return Err(ExecutionEnvelopeError::InvalidTerminalState {
                reason: format!("lifecycle {:?} is not terminal", analysis.lifecycle),
            });
        }
        Ok(())
    }

    /// Returns the validated lifecycle state.
    pub fn lifecycle_state(&self) -> Result<ExecutionLifecycleV1, ExecutionEnvelopeError> {
        self.analyze().map(|analysis| analysis.lifecycle)
    }

    /// Borrows the execution identity.
    #[must_use]
    pub const fn identity(&self) -> &ExecutionIdentityV1 {
        &self.identity
    }

    /// Borrows ordered execution records.
    #[must_use]
    pub fn records(&self) -> &[ExecutionRecordV1] {
        &self.records
    }

    /// Borrows the initial execution intent.
    #[must_use]
    pub fn intent(&self) -> Option<&ExecutionIntentV1> {
        self.records
            .first()
            .and_then(|record| match record.payload() {
                ExecutionRecordPayloadV1::Intent(intent) => Some(intent.as_ref()),
                _ => None,
            })
    }

    /// Borrows the authorization record when one exists.
    #[must_use]
    pub fn authorization(&self) -> Option<&AuthorizationRecordV1> {
        self.records
            .iter()
            .find_map(|record| match record.payload() {
                ExecutionRecordPayloadV1::Authorization(authorization) => {
                    Some(authorization.as_ref())
                }
                _ => None,
            })
    }

    /// Returns the terminal execution status when it maps to one.
    #[must_use]
    pub fn terminal_status(&self) -> Option<ExecutionStatusV1> {
        self.analyze().ok().and_then(|analysis| analysis.status)
    }

    fn analyze(&self) -> Result<EnvelopeAnalysis, ExecutionEnvelopeError> {
        if self.schema_version != EXECUTION_ENVELOPE_SCHEMA_V1 {
            return Err(ExecutionEnvelopeError::UnsupportedSchema {
                actual: self.schema_version,
            });
        }
        if self.records.is_empty() || self.records.len() > MAX_RECORDS {
            return Err(invalid_order(0, "envelope must contain 1 to 256 records"));
        }
        let serialized_size = serde_json::to_vec(self)
            .map_err(|error| ExecutionEnvelopeError::Canonicalization {
                reason: error.to_string(),
            })?
            .len();
        if serialized_size > MAX_ENVELOPE_BYTES {
            return Err(invalid_order(0, "serialized envelope exceeds 1 MiB"));
        }

        let mut seen_ids = std::collections::HashSet::new();
        let mut prior_digest: Option<&ContentDigestV1> = None;
        let mut prior_time: Option<&DateTime<Utc>> = None;
        for (index, record) in self.records.iter().enumerate() {
            let expected_sequence = u64::try_from(index).map_err(|_| {
                invalid_order(u64::MAX, "record index does not fit the V1 sequence")
            })?;
            if record.sequence() != expected_sequence {
                return Err(invalid_order(
                    record.sequence(),
                    "sequence gap or duplicate",
                ));
            }
            if !seen_ids.insert(record.record_id()) {
                return Err(invalid_order(record.sequence(), "duplicate record ID"));
            }
            if record.previous_digest() != prior_digest {
                return Err(ExecutionEnvelopeError::BrokenHashChain {
                    sequence: record.sequence(),
                });
            }
            if prior_time.is_some_and(|prior| record.recorded_at() < prior) {
                return Err(invalid_order(
                    record.sequence(),
                    "timestamps moved backwards",
                ));
            }
            record.verify_digest()?;
            prior_digest = Some(record.digest());
            prior_time = Some(record.recorded_at());
        }

        let ExecutionRecordPayloadV1::Intent(intent) = self.records[0].payload() else {
            return Err(invalid_order(0, "first record must be intent"));
        };
        let requested = &intent.requested_capabilities;
        let mut lifecycle = ExecutionLifecycleV1::Proposed;
        let mut grant: Option<&CapabilitySetV1> = None;
        let mut effective: Option<&CapabilitySetV1> = None;
        let mut admission_executor: Option<&ComponentIdentityV1> = None;
        let mut environment_recorded_at: Option<DateTime<Utc>> = None;
        let mut started_at: Option<DateTime<Utc>> = None;
        let mut observed_output_bytes = 0_u64;
        let mut terminal: Option<(&ExecutionRecordId, &ContentDigestV1)> = None;
        let mut status = None;

        for record in self.records.iter().skip(1) {
            match record.payload() {
                ExecutionRecordPayloadV1::Intent(_) => {
                    return Err(invalid_order(
                        record.sequence(),
                        "intent may appear only once",
                    ));
                }
                ExecutionRecordPayloadV1::Authorization(authorization) => {
                    if lifecycle != ExecutionLifecycleV1::Proposed {
                        return Err(invalid_order(
                            record.sequence(),
                            "authorization out of order",
                        ));
                    }
                    let statement = authorization.statement();
                    if statement.execution_identity() != &self.identity
                        || statement.intent_digest() != self.records[0].digest()
                    {
                        return Err(ExecutionEnvelopeError::InvalidAuthorization {
                            reason: "statement identity or intent digest mismatch".to_string(),
                        });
                    }
                    match statement.decision() {
                        AuthorizationDecisionV1::Allow { grant: allowed }
                        | AuthorizationDecisionV1::AllowWithConstraints { grant: allowed } => {
                            if !requested.contains(allowed) {
                                return Err(ExecutionEnvelopeError::CapabilityEscalation);
                            }
                            grant = Some(allowed);
                            lifecycle = ExecutionLifecycleV1::Authorized;
                        }
                        AuthorizationDecisionV1::Deny => {
                            lifecycle = ExecutionLifecycleV1::AuthorizationDenied;
                            terminal = Some((record.record_id(), record.digest()));
                            status = Some(ExecutionStatusV1::Denied);
                        }
                        AuthorizationDecisionV1::RequireReview { .. } => {
                            lifecycle = ExecutionLifecycleV1::ReviewRequired;
                        }
                        AuthorizationDecisionV1::RetryLater { .. } => {
                            lifecycle = ExecutionLifecycleV1::RetryLater;
                        }
                    }
                }
                ExecutionRecordPayloadV1::Admission(admission) => {
                    if lifecycle != ExecutionLifecycleV1::Authorized {
                        return Err(invalid_order(record.sequence(), "admission out of order"));
                    }
                    admission.validate()?;
                    match admission.decision {
                        AdmissionDecisionV1::Admitted => {
                            let admitted =
                                admission.effective_capabilities.as_ref().ok_or_else(|| {
                                    invalid_order(
                                        record.sequence(),
                                        "admitted capabilities missing",
                                    )
                                })?;
                            if !requested.contains(admitted)
                                || !grant.is_some_and(|allowed| allowed.contains(admitted))
                            {
                                return Err(ExecutionEnvelopeError::CapabilityEscalation);
                            }
                            effective = Some(admitted);
                            admission_executor = Some(&admission.executor);
                            lifecycle = ExecutionLifecycleV1::Admitted;
                        }
                        AdmissionDecisionV1::Denied => {
                            lifecycle = ExecutionLifecycleV1::AdmissionDenied;
                            terminal = Some((record.record_id(), record.digest()));
                            status = Some(ExecutionStatusV1::Denied);
                        }
                    }
                }
                ExecutionRecordPayloadV1::Environment(environment) => {
                    if lifecycle != ExecutionLifecycleV1::Admitted
                        || effective != Some(&environment.effective_capabilities)
                        || admission_executor != Some(&environment.executor)
                    {
                        return Err(invalid_order(
                            record.sequence(),
                            "environment differs from admitted capabilities",
                        ));
                    }
                    environment_recorded_at = Some(*record.recorded_at());
                    lifecycle = ExecutionLifecycleV1::Prepared;
                }
                ExecutionRecordPayloadV1::Started(started) => {
                    if lifecycle != ExecutionLifecycleV1::Prepared
                        || started.started_at > *record.recorded_at()
                        || environment_recorded_at
                            .is_none_or(|environment_time| started.started_at < environment_time)
                    {
                        return Err(invalid_order(record.sequence(), "start out of order"));
                    }
                    started_at = Some(started.started_at);
                    lifecycle = ExecutionLifecycleV1::Running;
                }
                ExecutionRecordPayloadV1::Output(output) => {
                    if lifecycle != ExecutionLifecycleV1::Running {
                        return Err(invalid_order(
                            record.sequence(),
                            "output requires running workload",
                        ));
                    }
                    output.validate()?;
                    observed_output_bytes = observed_output_bytes
                        .checked_add(output.observed_bytes)
                        .ok_or_else(|| {
                            invalid_order(record.sequence(), "cumulative output byte overflow")
                        })?;
                    if effective
                        .and_then(|capabilities| capabilities.resources().output_bytes)
                        .is_some_and(|limit| observed_output_bytes > limit)
                    {
                        return Err(invalid_order(
                            record.sequence(),
                            "cumulative output exceeds effective byte limit",
                        ));
                    }
                    if !intent.expected_output.media_types.is_empty()
                        && !intent
                            .expected_output
                            .media_types
                            .contains(&output.content.media_type)
                    {
                        return Err(invalid_order(
                            record.sequence(),
                            "output media type is not accepted by the output contract",
                        ));
                    }
                }
                ExecutionRecordPayloadV1::Artifact(artifact) => {
                    if lifecycle != ExecutionLifecycleV1::Running {
                        return Err(invalid_order(
                            record.sequence(),
                            "artifact requires running workload",
                        ));
                    }
                    artifact.validate()?;
                }
                ExecutionRecordPayloadV1::Result(result) => {
                    result.validate()?;
                    let permitted = match result.status {
                        ExecutionStatusV1::Succeeded => lifecycle == ExecutionLifecycleV1::Running,
                        ExecutionStatusV1::Failed
                        | ExecutionStatusV1::Cancelled
                        | ExecutionStatusV1::TimedOut => matches!(
                            lifecycle,
                            ExecutionLifecycleV1::Admitted
                                | ExecutionLifecycleV1::Prepared
                                | ExecutionLifecycleV1::Running
                        ),
                        ExecutionStatusV1::Denied => false,
                    };
                    let valid_phase = match (lifecycle, result.error.as_ref()) {
                        (ExecutionLifecycleV1::Admitted, Some(error)) => {
                            error.phase == ExecutionFailurePhaseV1::Preparation
                        }
                        (ExecutionLifecycleV1::Prepared, Some(error)) => matches!(
                            error.phase,
                            ExecutionFailurePhaseV1::Preparation | ExecutionFailurePhaseV1::Spawn
                        ),
                        (ExecutionLifecycleV1::Running, Some(error)) => matches!(
                            error.phase,
                            ExecutionFailurePhaseV1::Runtime | ExecutionFailurePhaseV1::Cleanup
                        ),
                        (ExecutionLifecycleV1::Running, None)
                            if result.status == ExecutionStatusV1::Succeeded =>
                        {
                            true
                        }
                        _ => false,
                    };
                    let observation_within_contract =
                        result.observation.as_ref().is_none_or(|value| {
                            serde_json::to_vec(value).is_ok_and(|bytes| {
                                u64::try_from(bytes.len()).is_ok_and(|size| {
                                    size <= intent.expected_output.max_inline_bytes
                                })
                            })
                        });
                    let completed_after_start =
                        started_at.is_none_or(|started| result.completed_at >= started);
                    if !permitted
                        || !valid_phase
                        || !observation_within_contract
                        || !completed_after_start
                        || result.completed_at < *self.records[0].recorded_at()
                        || result.completed_at > *record.recorded_at()
                    {
                        return Err(invalid_order(record.sequence(), "result out of order"));
                    }
                    lifecycle = ExecutionLifecycleV1::Completed;
                    terminal = Some((record.record_id(), record.digest()));
                    status = Some(result.status);
                }
                ExecutionRecordPayloadV1::Verification(verification) => {
                    let Some((terminal_id, terminal_digest)) = terminal else {
                        return Err(invalid_order(
                            record.sequence(),
                            "verification requires prior terminal record",
                        ));
                    };
                    if verification.subject_record_id != *terminal_id
                        || verification.subject_digest != *terminal_digest
                    {
                        return Err(invalid_order(
                            record.sequence(),
                            "verification subject does not match terminal record",
                        ));
                    }
                    lifecycle = ExecutionLifecycleV1::Verified;
                }
            }
        }

        let complete = matches!(
            lifecycle,
            ExecutionLifecycleV1::AuthorizationDenied
                | ExecutionLifecycleV1::ReviewRequired
                | ExecutionLifecycleV1::RetryLater
                | ExecutionLifecycleV1::AdmissionDenied
                | ExecutionLifecycleV1::Completed
                | ExecutionLifecycleV1::Verified
        );
        Ok(EnvelopeAnalysis {
            lifecycle,
            complete,
            status,
        })
    }
}

impl<'de> Deserialize<'de> for ExecutionEnvelopeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EnvelopeWire {
            schema_version: u32,
            identity: ExecutionIdentityV1,
            records: Vec<ExecutionRecordV1>,
        }
        let wire = EnvelopeWire::deserialize(deserializer)?;
        let envelope = Self {
            schema_version: wire.schema_version,
            identity: wire.identity,
            records: wire.records,
        };
        envelope.validate_prefix().map_err(de::Error::custom)?;
        Ok(envelope)
    }
}

struct EnvelopeAnalysis {
    lifecycle: ExecutionLifecycleV1,
    complete: bool,
    status: Option<ExecutionStatusV1>,
}

fn invalid_order(sequence: u64, reason: impl Into<String>) -> ExecutionEnvelopeError {
    ExecutionEnvelopeError::InvalidRecordOrder {
        sequence,
        reason: reason.into(),
    }
}

struct UniqueJsonValue(serde_json::Value);

impl<'de> Deserialize<'de> for UniqueJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UniqueValueVisitor;

        impl<'de> de::Visitor<'de> for UniqueValueVisitor {
            type Value = UniqueJsonValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON value without duplicate object keys")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Null))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_unit()
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Bool(value)))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Number(value.into())))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::Number(value.into())))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                serde_json::Number::from_f64(value)
                    .map(serde_json::Value::Number)
                    .map(UniqueJsonValue)
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::String(
                    value.to_string(),
                )))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(serde_json::Value::String(value)))
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<UniqueJsonValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueJsonValue(serde_json::Value::Array(values)))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!(
                            "duplicate JSON object key '{key}'"
                        )));
                    }
                    let value = map.next_value::<UniqueJsonValue>()?;
                    values.insert(key, value.0);
                }
                Ok(UniqueJsonValue(serde_json::Value::Object(values)))
            }
        }

        deserializer.deserialize_any(UniqueValueVisitor)
    }
}

/// Admits JSON under the V1 safe-integer, integer-only JCS profile.
pub fn canonicalize_json_v1(input: &[u8]) -> Result<Vec<u8>, ExecutionEnvelopeError> {
    let options = JcsOptions::ijson()
        .max_depth(MAX_JSON_DEPTH)
        .max_bytes(Some(1024 * 1024))
        .integers_only(true);
    admit_with(input, &options).map_err(|error| ExecutionEnvelopeError::Canonicalization {
        reason: error.to_string(),
    })
}

fn record_digest(
    record_id: &ExecutionRecordId,
    sequence: u64,
    recorded_at: &DateTime<Utc>,
    previous_digest: Option<&ContentDigestV1>,
    payload: &ExecutionRecordPayloadV1,
) -> Result<ContentDigestV1, ExecutionEnvelopeError> {
    let projection = RecordDigestProjection {
        record_id,
        sequence,
        recorded_at: format_timestamp(recorded_at),
        previous_digest,
        payload,
    };
    let raw = serde_json::to_vec(&projection).map_err(|error| {
        ExecutionEnvelopeError::Canonicalization {
            reason: error.to_string(),
        }
    })?;
    let canonical = canonicalize_json_v1(&raw)?;
    let mut framed = b"crux.execution.record.v1\0".to_vec();
    framed.extend_from_slice(&canonical);
    Ok(ContentDigestV1::digest(&framed))
}

fn format_timestamp(timestamp: &DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn parse_exact_timestamp(value: &str) -> Result<DateTime<Utc>, ExecutionEnvelopeError> {
    let timestamp = DateTime::parse_from_rfc3339(value)
        .map_err(|error| invalid_authorization(error.to_string()))?
        .with_timezone(&Utc);
    if format_timestamp(&timestamp) != value {
        return Err(invalid_authorization(
            "timestamp must use UTC and nine fractional digits",
        ));
    }
    Ok(timestamp)
}

const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_COLLECTION_LENGTH: usize = 256;
const MAX_ACTION_INPUT_BYTES: usize = 256 * 1024;

fn validate_json(value: &serde_json::Value) -> Result<(), ExecutionEnvelopeError> {
    validate_json_at_depth(value, 0)?;
    let size = serde_json::to_vec(value)
        .map_err(|error| invalid_action("input", error.to_string()))?
        .len();
    if size > MAX_ACTION_INPUT_BYTES {
        return Err(invalid_action("input", "serialized input exceeds 256 KiB"));
    }
    Ok(())
}

fn validate_json_at_depth(
    value: &serde_json::Value,
    depth: usize,
) -> Result<(), ExecutionEnvelopeError> {
    if depth > MAX_JSON_DEPTH {
        return Err(invalid_action("input", "JSON nesting exceeds 32"));
    }
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) => Ok(()),
        serde_json::Value::Number(number) => match number.as_u64() {
            Some(value) if value <= MAX_SAFE_JSON_INTEGER => Ok(()),
            _ => Err(invalid_action(
                "input",
                "numbers must be safe non-negative integers",
            )),
        },
        serde_json::Value::String(value) => validate_nonempty_or_empty_bounded("input", value),
        serde_json::Value::Array(values) => {
            if values.len() > MAX_JSON_COLLECTION_LENGTH {
                return Err(invalid_action("input", "JSON array exceeds 256 entries"));
            }
            values
                .iter()
                .try_for_each(|value| validate_json_at_depth(value, depth + 1))
        }
        serde_json::Value::Object(values) => {
            if values.len() > MAX_JSON_COLLECTION_LENGTH {
                return Err(invalid_action("input", "JSON object exceeds 256 entries"));
            }
            for (key, value) in values {
                validate_nonempty_string("input key", key)?;
                validate_json_at_depth(value, depth + 1)?;
            }
            Ok(())
        }
    }
}

fn validate_nonempty_string(field: &str, value: &str) -> Result<(), ExecutionEnvelopeError> {
    if value.is_empty() || value.len() > MAX_STRING_LENGTH || value.trim() != value {
        return Err(invalid_action(
            field,
            "must be non-empty, unpadded, and at most 4 KiB",
        ));
    }
    Ok(())
}

fn validate_nonempty_or_empty_bounded(
    field: &str,
    value: &str,
) -> Result<(), ExecutionEnvelopeError> {
    if value.len() > MAX_STRING_LENGTH {
        return Err(invalid_action(field, "must be at most 4 KiB"));
    }
    Ok(())
}

fn validate_safe_integer(field: &str, value: u64) -> Result<(), ExecutionEnvelopeError> {
    if value > MAX_SAFE_JSON_INTEGER {
        return Err(invalid_action(field, "must fit the JCS safe-integer range"));
    }
    Ok(())
}

fn sort_unique_strings(field: &str, values: &mut [String]) -> Result<(), ExecutionEnvelopeError> {
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid_capability(field, "duplicate value"));
    }
    Ok(())
}

fn is_normalized_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        && (path == "/" || !path.ends_with('/'))
        && !path.contains("//")
        && !path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
}

const fn limit_contains(limit: Option<u64>, candidate: Option<u64>) -> bool {
    match (limit, candidate) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(limit), Some(candidate)) => candidate <= limit,
    }
}

fn exact_contains(limit: Option<u64>, candidate: Option<u64>) -> bool {
    limit == candidate
}

fn invalid_action(field: &str, reason: impl Into<String>) -> ExecutionEnvelopeError {
    ExecutionEnvelopeError::InvalidAction {
        field: field.to_string(),
        reason: reason.into(),
    }
}

fn invalid_capability(field: &str, reason: impl Into<String>) -> ExecutionEnvelopeError {
    ExecutionEnvelopeError::InvalidCapability {
        field: field.to_string(),
        reason: reason.into(),
    }
}

fn invalid_authorization(reason: impl Into<String>) -> ExecutionEnvelopeError {
    ExecutionEnvelopeError::InvalidAuthorization {
        reason: reason.into(),
    }
}

fn evidence_sort_key(evidence: &EvidenceRefV1) -> (&str, &str, u64, Option<&str>) {
    (
        &evidence.media_type,
        evidence.digest.value(),
        evidence.size_bytes,
        evidence.locator.as_deref(),
    )
}

fn validate_prefixed_ulid(
    value: &str,
    prefix: &str,
    field: IdentityFieldV1,
) -> Result<(), ExecutionEnvelopeError> {
    let Some(ulid) = value.strip_prefix(prefix) else {
        return Err(ExecutionEnvelopeError::InvalidIdentity { field });
    };
    if ulid.len() != ULID_TEXT_LENGTH || ulid != ulid.to_ascii_uppercase() {
        return Err(ExecutionEnvelopeError::InvalidIdentity { field });
    }
    Ulid::from_str(ulid)
        .map(|_| ())
        .map_err(|_| ExecutionEnvelopeError::InvalidIdentity { field })
}
