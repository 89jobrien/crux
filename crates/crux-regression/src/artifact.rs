use std::{fmt, str::FromStr};

use crux_improve::Crux;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::RegressionError;

const DIGEST_PREFIX: &str = "sha256:";
const DIGEST_DOMAIN: &[u8] = b"crux-trace-artifact:v1\0";
pub(crate) const TRACE_FORMAT_VERSION: u16 = 1;
pub(crate) const DIGEST_ALGORITHM: &str = "sha256";

/// Safe filesystem component identifying one regression scenario.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RegressionCaseId(String);

impl RegressionCaseId {
    /// Validates and constructs a path-safe regression case identifier.
    ///
    /// # Errors
    ///
    /// Returns [`RegressionError::InvalidCaseId`] for empty or unsafe values.
    pub fn new(value: impl Into<String>) -> Result<Self, RegressionError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value != "."
            && value != ".."
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        if valid {
            Ok(Self(value))
        } else {
            Err(RegressionError::InvalidCaseId(value))
        }
    }

    /// Borrows the validated identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RegressionCaseId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for RegressionCaseId {
    type Err = RegressionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for RegressionCaseId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

impl AsRef<str> for RegressionCaseId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// SHA-256 identity of one canonical raw trace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraceDigest([u8; 32]);

impl TraceDigest {
    #[cfg(test)]
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrows the raw SHA-256 digest bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for TraceDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{DIGEST_PREFIX}{}", self.hex())
    }
}

impl FromStr for TraceDigest {
    type Err = RegressionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(hex) = value.strip_prefix(DIGEST_PREFIX) else {
            return Err(RegressionError::InvalidDigest(value.into()));
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(RegressionError::InvalidDigest(value.into()));
        }
        let mut bytes = [0_u8; 32];
        for (index, output) in bytes.iter_mut().enumerate() {
            *output = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
                .map_err(|_| RegressionError::InvalidDigest(value.into()))?;
        }
        Ok(Self(bytes))
    }
}

impl AsRef<[u8]> for TraceDigest {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Serialize for TraceDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TraceDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// ULID identifying one persisted regression report.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RegressionRunId(String);

impl RegressionRunId {
    /// Generates a new ULID-backed report identifier.
    pub fn new() -> Self {
        Self(ulid::Ulid::new().to_string())
    }

    /// Borrows the ULID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TraceEnvelope {
    pub(crate) format_version: u16,
    pub(crate) digest_algorithm: String,
    pub(crate) trace: Crux<Value>,
}

impl TraceEnvelope {
    pub(crate) fn new(trace: Crux<Value>) -> Self {
        Self {
            format_version: TRACE_FORMAT_VERSION,
            digest_algorithm: DIGEST_ALGORITHM.into(),
            trace,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), RegressionError> {
        if self.format_version != TRACE_FORMAT_VERSION {
            return Err(RegressionError::UnsupportedFormatVersion(
                self.format_version,
            ));
        }
        if self.digest_algorithm != DIGEST_ALGORITHM {
            return Err(RegressionError::UnsupportedDigestAlgorithm(
                self.digest_algorithm.clone(),
            ));
        }
        Ok(())
    }
}

impl Default for RegressionRunId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RegressionRunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for RegressionRunId {
    type Err = RegressionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        ulid::Ulid::from_string(value)
            .map(|_| Self(value.into()))
            .map_err(|_| RegressionError::InvalidRunId(value.into()))
    }
}

impl<'de> Deserialize<'de> for RegressionRunId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

impl AsRef<str> for RegressionRunId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Computes a stable V1 SHA-256 identity for a complete raw trace.
pub fn digest_trace(trace: &Crux<Value>) -> Result<TraceDigest, RegressionError> {
    let canonical = canonicalize(serde_json::to_value(trace)?);
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_DOMAIN);
    hasher.update(serde_json::to_vec(&canonical)?);
    Ok(TraceDigest(hasher.finalize().into()))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted = object
                .into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect();
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests {
    use proptest::{prelude::any, prop_assert_eq, prop_assert_ne, proptest};

    use super::*;

    #[test]
    fn case_id_accepts_only_safe_ascii_components() {
        assert!(RegressionCaseId::new("nightly-showcase").is_ok());
        for invalid in ["", ".", "..", "../escape", "two words", "semi;colon"] {
            assert!(RegressionCaseId::new(invalid).is_err(), "{invalid:?}");
        }
        assert!(serde_json::from_str::<RegressionCaseId>("\"../escape\"").is_err());
    }

    #[test]
    fn trace_digest_display_parse_and_serde_round_trip() {
        let digest = TraceDigest::from_bytes([0xab; 32]);
        let encoded = digest.to_string();
        assert_eq!(encoded.len(), "sha256:".len() + 64);
        assert_eq!(TraceDigest::from_str(&encoded).unwrap(), digest);

        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(serde_json::from_str::<TraceDigest>(&json).unwrap(), digest);
    }

    #[test]
    fn canonical_digest_ignores_json_object_insertion_order() {
        let mut left = crux_schema_for_test();
        let mut left_map = serde_json::Map::new();
        left_map.insert("second".into(), serde_json::json!(2));
        left_map.insert("first".into(), serde_json::json!(1));
        left.value = Ok(Value::Object(left_map));
        let mut right = left.clone();
        let mut right_map = serde_json::Map::new();
        right_map.insert("first".into(), serde_json::json!(1));
        right_map.insert("second".into(), serde_json::json!(2));
        right.value = Ok(Value::Object(right_map));

        assert_eq!(digest_trace(&left).unwrap(), digest_trace(&right).unwrap());
    }

    #[test]
    fn canonical_digest_has_stable_v1_golden_vector() {
        let digest = digest_trace(&crux_schema_for_test()).unwrap();
        assert_eq!(
            digest.to_string(),
            "sha256:fbcf41ae61f7f060d91840f7aaeb99c7ff27d7e85667cbc90733fe0fa9e7e5c9"
        );
    }

    proptest! {
        #[test]
        fn trace_digest_serde_round_trips_arbitrary_bytes(bytes in any::<[u8; 32]>()) {
            let digest = TraceDigest::from_bytes(bytes);
            let json = serde_json::to_string(&digest).unwrap();
            let restored: TraceDigest = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(restored, digest);
        }

        #[test]
        fn trace_value_mutation_changes_digest(answer in any::<i32>()) {
            let mut baseline = crux_schema_for_test();
            baseline.value = Ok(serde_json::json!({"answer": i64::from(answer)}));
            let mut candidate = baseline.clone();
            candidate.value = Ok(serde_json::json!({"answer": i64::from(answer) + 1}));

            prop_assert_ne!(digest_trace(&baseline).unwrap(), digest_trace(&candidate).unwrap());
        }
    }

    fn crux_schema_for_test() -> Crux<Value> {
        Crux {
            id: serde_json::from_str("\"crux_01ARZ3NDEKTSV4RRFFQ69G5FAV\"").unwrap(),
            agent: "artifact-test".into(),
            pipeline_version: None,
            value: Ok(serde_json::json!({"answer": 42})),
            steps: Vec::new(),
            children: Vec::new(),
            started_at: chrono::DateTime::UNIX_EPOCH,
            finished_at: Some(chrono::DateTime::UNIX_EPOCH),
        }
    }
}
