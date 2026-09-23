use std::str::FromStr;

use chrono::{DateTime, Utc};
use crux_types::execution::{
    ActionSpecV1, ActorIdentityV1, ActorKindV1, AdmissionRecordV1, AuthorizationDecisionV1,
    AuthorizationNonce, AuthorizationProofV1, AuthorizationRecordV1, AuthorizationStatementV1,
    CapabilitySetV1, ComponentIdentityV1, ContentDigestV1, DecisionReasonV1, DigestAlgorithmV1,
    EXECUTION_ENVELOPE_SCHEMA_V1, EvidenceRefV1, ExecutionEnvelopeV1, ExecutionEnvironmentV1,
    ExecutionErrorV1, ExecutionFailurePhaseV1, ExecutionId, ExecutionIdentityV1, ExecutionIntentV1,
    ExecutionLifecycleV1, ExecutionOutputV1, ExecutionRecordId, ExecutionRecordPayloadV1,
    ExecutionRecordV1, ExecutionResultV1, ExecutionStartedV1, ExecutionStatusV1,
    FilesystemAccessV1, FilesystemScopeV1, NetworkCapabilityV1, NetworkEndpointV1,
    OutputContractV1, OutputStreamV1, PolicyIdentityV1, ProcessCapabilityV1, ReplayMetadataV1,
    ResourceLimitsV1, SecretRefV1, SideEffectClassV1, SignatureAlgorithmV1, VerificationRecordV1,
    VerificationStatusV1, canonicalize_json_v1,
};
use crux_types::id::CruxId;
use proptest::prelude::*;
use serde_json::json;
use ulid::Ulid;

const ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[test]
fn execution_module_exposes_v1_schema() {
    assert_eq!(EXECUTION_ENVELOPE_SCHEMA_V1, 1);
}

#[test]
fn execution_identifiers_use_exact_prefixes_and_uppercase_ulids() {
    let execution = ExecutionId::from_str(&format!("exec_{ULID}")).expect("valid execution ID");
    let record = ExecutionRecordId::from_str(&format!("rec_{ULID}")).expect("valid record ID");

    assert_eq!(execution.as_str(), format!("exec_{ULID}"));
    assert_eq!(record.as_str(), format!("rec_{ULID}"));
    assert!(ExecutionId::new().as_str().starts_with("exec_"));
    assert!(ExecutionRecordId::new().as_str().starts_with("rec_"));
}

#[test]
fn execution_identifiers_reject_wrong_prefix_and_lowercase_ulids() {
    assert!(ExecutionId::from_str(&format!("crux_{ULID}")).is_err());
    assert!(ExecutionRecordId::from_str(&format!("record_{ULID}")).is_err());
    assert!(ExecutionId::from_str(&format!("exec_{}", ULID.to_lowercase())).is_err());
    assert!(ExecutionRecordId::from_str(&format!("rec_{}", ULID.to_lowercase())).is_err());
}

#[test]
fn authorization_nonce_rejects_empty_or_padded_values() {
    assert!(AuthorizationNonce::new("").is_err());
    assert!(AuthorizationNonce::new(" nonce").is_err());
    assert!(AuthorizationNonce::new("nonce ").is_err());
    assert_eq!(
        AuthorizationNonce::new("nonce-1")
            .expect("valid nonce")
            .as_str(),
        "nonce-1"
    );
}

#[test]
fn sha256_digest_requires_exact_lowercase_hex() {
    let valid = "ab".repeat(32);
    let digest = ContentDigestV1::sha256(valid.clone()).expect("valid digest");

    assert_eq!(digest.algorithm(), DigestAlgorithmV1::Sha256);
    assert_eq!(digest.value(), valid);
    assert!(ContentDigestV1::sha256("ab").is_err());
    assert!(ContentDigestV1::sha256("AB".repeat(32)).is_err());
    assert!(ContentDigestV1::sha256("zz".repeat(32)).is_err());
}

#[test]
fn identity_rejects_self_duplicate_and_unsorted_parents() {
    let execution = ExecutionId::from_str(&format!("exec_{ULID}")).expect("valid execution ID");
    let parent_a =
        ExecutionId::from_str("exec_01ARZ3NDEKTSV4RRFFQ69G5FAW").expect("valid parent A");
    let parent_b =
        ExecutionId::from_str("exec_01ARZ3NDEKTSV4RRFFQ69G5FAX").expect("valid parent B");
    let actor = ActorIdentityV1::new(ActorKindV1::Agent, "looprs").expect("valid actor");

    assert!(
        ExecutionIdentityV1::new(
            execution.clone(),
            None,
            vec![execution.clone()],
            actor.clone(),
        )
        .is_err()
    );
    assert!(
        ExecutionIdentityV1::new(
            execution.clone(),
            None,
            vec![parent_a.clone(), parent_a.clone()],
            actor.clone(),
        )
        .is_err()
    );
    assert!(ExecutionIdentityV1::new(execution, None, vec![parent_b, parent_a], actor).is_err());
}

#[test]
fn identity_round_trips_and_rejects_invalid_wire_ids() {
    let execution = ExecutionId::from_str(&format!("exec_{ULID}")).expect("valid execution ID");
    let actor = ActorIdentityV1::new(ActorKindV1::Service, "rulery").expect("valid actor");
    let identity = ExecutionIdentityV1::new(execution, None, Vec::new(), actor)
        .expect("valid execution identity");
    let json = serde_json::to_string(&identity).expect("serialize identity");
    let decoded: ExecutionIdentityV1 = serde_json::from_str(&json).expect("deserialize identity");

    assert_eq!(decoded, identity);
    assert!(
        serde_json::from_str::<ExecutionId>(&format!("\"exec_{}\"", ULID.to_lowercase())).is_err()
    );
}

#[test]
fn action_rejects_non_canonical_numbers_and_excessive_depth() {
    let output = OutputContractV1::new(None, vec!["application/json".into()], 1024)
        .expect("valid output contract");
    let action = |input| {
        ActionSpecV1::new(
            "minibox.container",
            "run.v1",
            input,
            None,
            SideEffectClassV1::Reversible,
            None,
        )
    };

    assert!(action(json!({ "value": -1 })).is_err());
    assert!(action(json!({ "value": 1.5 })).is_err());
    assert!(action(json!({ "value": 9_007_199_254_740_992_u64 })).is_err());

    let mut deep = json!(null);
    for _ in 0..33 {
        deep = json!([deep]);
    }
    assert!(action(deep).is_err());
    assert_eq!(output.max_inline_bytes(), 1024);
}

#[test]
fn filesystem_scope_requires_normalized_absolute_paths() {
    assert!(FilesystemScopeV1::new("relative", FilesystemAccessV1::Read).is_err());
    assert!(FilesystemScopeV1::new("/tmp/../etc", FilesystemAccessV1::Read).is_err());
    assert!(FilesystemScopeV1::new("/tmp//repo", FilesystemAccessV1::Read).is_err());
    assert!(FilesystemScopeV1::new("/tmp/", FilesystemAccessV1::Read).is_err());
    assert!(FilesystemScopeV1::new("/tmp/repo", FilesystemAccessV1::ReadWrite).is_ok());
}

#[test]
fn capability_containment_is_most_restrictive() {
    let request = CapabilitySetV1::new(
        vec![
            FilesystemScopeV1::new("/workspace", FilesystemAccessV1::ReadWrite)
                .expect("request scope"),
        ],
        NetworkCapabilityV1::Unrestricted,
        vec!["RUST_LOG".into()],
        vec![SecretRefV1::new("api", "op://example").expect("secret ref")],
        ProcessCapabilityV1::new(true),
        ResourceLimitsV1::new(
            Some(60_000),
            Some(1_000_000),
            Some(100),
            Some(64),
            Some(1_000_000),
        )
        .expect("request resources"),
    )
    .expect("request capability set");
    let grant = CapabilitySetV1::new(
        vec![
            FilesystemScopeV1::new("/workspace/repo", FilesystemAccessV1::Read)
                .expect("grant scope"),
        ],
        NetworkCapabilityV1::Denied,
        Vec::new(),
        Vec::new(),
        ProcessCapabilityV1::new(false),
        ResourceLimitsV1::new(
            Some(30_000),
            Some(500_000),
            Some(100),
            Some(32),
            Some(500_000),
        )
        .expect("grant resources"),
    )
    .expect("grant capability set");

    assert!(request.contains(&grant));
    assert!(!grant.contains(&request));
    assert!(request.contains(&request));
}

#[test]
fn network_allowlist_requires_endpoint_subset() {
    let https = NetworkEndpointV1::new("https", "example.com", Some(443)).expect("endpoint");
    let api = NetworkEndpointV1::new("https", "api.example.com", Some(443)).expect("endpoint");
    let request = NetworkCapabilityV1::allow_list(vec![https.clone(), api]).expect("allowlist");
    let grant = NetworkCapabilityV1::allow_list(vec![https]).expect("allowlist");

    assert!(request.contains(&grant));
    assert!(!grant.contains(&request));
    assert!(request.contains(&NetworkCapabilityV1::Denied));
}

#[test]
fn resource_containment_treats_cpu_weight_as_exact() {
    let request = ResourceLimitsV1::new(Some(100), Some(100), Some(100), Some(10), Some(100))
        .expect("request resources");
    let narrower = ResourceLimitsV1::new(Some(50), Some(50), Some(100), Some(5), Some(50))
        .expect("narrower resources");
    let different_cpu = ResourceLimitsV1::new(Some(50), Some(50), Some(99), Some(5), Some(50))
        .expect("different CPU resources");

    assert!(request.contains(&narrower));
    assert!(!request.contains(&different_cpu));
}

#[test]
fn capability_set_rejects_duplicates_and_unknown_wire_fields() {
    let duplicate = vec![
        FilesystemScopeV1::new("/workspace", FilesystemAccessV1::Read).expect("scope"),
        FilesystemScopeV1::new("/workspace", FilesystemAccessV1::Read).expect("scope"),
    ];
    assert!(
        CapabilitySetV1::new(
            duplicate,
            NetworkCapabilityV1::Denied,
            Vec::new(),
            Vec::new(),
            ProcessCapabilityV1::new(false),
            ResourceLimitsV1::default(),
        )
        .is_err()
    );

    let wire = r#"{
        "filesystem": [],
        "network": "denied",
        "environment_names": [],
        "secrets": [],
        "process": { "privileged": false },
        "resources": {},
        "unexpected": true
    }"#;
    assert!(serde_json::from_str::<CapabilitySetV1>(wire).is_err());
}

proptest! {
    #[test]
    fn capability_containment_is_transitive(
        first in 0_u64..1_000_000,
        second in 0_u64..1_000_000,
        third in 0_u64..1_000_000,
    ) {
        let mut limits = [first, second, third];
        limits.sort_unstable_by(|left, right| right.cmp(left));
        let capability = |memory| {
            CapabilitySetV1::new(
                Vec::new(),
                NetworkCapabilityV1::Denied,
                Vec::new(),
                Vec::new(),
                ProcessCapabilityV1::new(false),
                ResourceLimitsV1::new(None, Some(memory), None, None, None)
                    .expect("generated safe resource limit"),
            )
            .expect("generated capability set")
        };
        let broad = capability(limits[0]);
        let middle = capability(limits[1]);
        let narrow = capability(limits[2]);

        prop_assert!(broad.contains(&middle));
        prop_assert!(middle.contains(&narrow));
        prop_assert!(broad.contains(&narrow));
        prop_assert!(broad.contains(&broad));
    }
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-22T00:00:00Z")
        .expect("fixed timestamp")
        .with_timezone(&Utc)
}

fn sample_intent(input: serde_json::Value) -> ExecutionIntentV1 {
    ExecutionIntentV1 {
        action: ActionSpecV1::new(
            "minibox.container",
            "run.v1",
            input,
            None,
            SideEffectClassV1::Reversible,
            Some("repair-1".into()),
        )
        .expect("sample action"),
        requested_capabilities: CapabilitySetV1::new(
            Vec::new(),
            NetworkCapabilityV1::Denied,
            Vec::new(),
            Vec::new(),
            ProcessCapabilityV1::new(false),
            ResourceLimitsV1::default(),
        )
        .expect("sample capabilities"),
        expected_output: OutputContractV1::new(None, vec!["application/json".into()], 1024)
            .expect("sample output contract"),
    }
}

#[test]
fn record_hash_matches_golden_vector() {
    let record_id = ExecutionRecordId::from_str(&format!("rec_{ULID}")).expect("record ID");
    let record = ExecutionRecordV1::new(
        record_id,
        0,
        fixed_time(),
        None,
        ExecutionRecordPayloadV1::Intent(Box::new(sample_intent(json!({ "a": 1, "b": 2 })))),
    )
    .expect("record");

    assert_eq!(
        record.digest().value(),
        "32aac6a9c676337b6897d2ed62e3882a5d071234cffc10ff64564c8b2a0f7e41"
    );
}

#[test]
fn record_golden_fixture_round_trips() {
    let fixture = include_str!("fixtures/execution-record-v1.json");
    let record: ExecutionRecordV1 = serde_json::from_str(fixture).expect("valid golden record");
    let encoded = serde_json::to_value(&record).expect("serialize golden record");
    let expected: serde_json::Value = serde_json::from_str(fixture).expect("golden JSON");

    assert_eq!(encoded, expected);
    assert_eq!(
        canonicalize_json_v1(fixture.as_bytes()).expect("canonical record bytes"),
        include_str!("fixtures/execution-record-v1.jcs")
            .trim_end()
            .as_bytes()
    );
}

#[test]
fn record_hash_is_independent_of_input_map_order() {
    let first_input: serde_json::Value =
        serde_json::from_str(r#"{"b":2,"a":1}"#).expect("first input");
    let second_input: serde_json::Value =
        serde_json::from_str(r#"{"a":1,"b":2}"#).expect("second input");
    let record_id = ExecutionRecordId::from_str(&format!("rec_{ULID}")).expect("record ID");
    let first = ExecutionRecordV1::new(
        record_id.clone(),
        0,
        fixed_time(),
        None,
        ExecutionRecordPayloadV1::Intent(Box::new(sample_intent(first_input))),
    )
    .expect("first record");
    let second = ExecutionRecordV1::new(
        record_id,
        0,
        fixed_time(),
        None,
        ExecutionRecordPayloadV1::Intent(Box::new(sample_intent(second_input))),
    )
    .expect("second record");

    assert_eq!(first.digest(), second.digest());
}

#[test]
fn record_deserialization_rejects_tampered_payload() {
    let record = ExecutionRecordV1::new(
        ExecutionRecordId::from_str(&format!("rec_{ULID}")).expect("record ID"),
        0,
        fixed_time(),
        None,
        ExecutionRecordPayloadV1::Intent(Box::new(sample_intent(json!({ "a": 1 })))),
    )
    .expect("record");
    let mut wire = serde_json::to_value(record).expect("record value");
    wire["payload"]["value"]["action"]["input"]["a"] = json!(2);

    assert!(serde_json::from_value::<ExecutionRecordV1>(wire).is_err());
}

#[test]
fn canonical_admission_rejects_duplicate_keys_and_unsafe_numbers() {
    assert!(canonicalize_json_v1(br#"{"a":1,"a":2}"#).is_err());
    assert!(canonicalize_json_v1(b"9007199254740992").is_err());
    assert!(canonicalize_json_v1(b"1.5").is_err());
    assert_eq!(
        canonicalize_json_v1(br#"{ "b": 2, "a": 1 }"#).expect("canonical JSON"),
        br#"{"a":1,"b":2}"#
    );
}

fn sample_identity() -> ExecutionIdentityV1 {
    ExecutionIdentityV1::new(
        ExecutionId::from_str(&format!("exec_{ULID}")).expect("execution ID"),
        None,
        Vec::new(),
        ActorIdentityV1::new(ActorKindV1::Agent, "looprs").expect("actor"),
    )
    .expect("identity")
}

fn empty_capabilities() -> CapabilitySetV1 {
    CapabilitySetV1::new(
        Vec::new(),
        NetworkCapabilityV1::Denied,
        Vec::new(),
        Vec::new(),
        ProcessCapabilityV1::new(false),
        ResourceLimitsV1::default(),
    )
    .expect("empty capabilities")
}

fn sample_authorization_statement() -> AuthorizationStatementV1 {
    AuthorizationStatementV1::new(
        sample_identity(),
        ContentDigestV1::sha256("11".repeat(32)).expect("intent digest"),
        ActorIdentityV1::new(ActorKindV1::Service, "rulery").expect("authority"),
        PolicyIdentityV1::new(
            "workspace-policy",
            "1.0.0",
            ContentDigestV1::sha256("22".repeat(32)).expect("policy digest"),
        )
        .expect("policy"),
        AuthorizationDecisionV1::Allow {
            grant: empty_capabilities(),
        },
        vec![DecisionReasonV1::new("policy.allow", "request satisfies policy").expect("reason")],
        SignatureAlgorithmV1::Ed25519,
        "rulery-key-1",
        "minibox://native-test",
        fixed_time(),
        fixed_time() + chrono::Duration::minutes(5),
        AuthorizationNonce::new("nonce-authorization-1").expect("nonce"),
    )
    .expect("statement")
}

#[test]
fn authorization_statement_signing_digest_matches_golden() {
    let statement = sample_authorization_statement();
    let signing_bytes = statement.signing_bytes().expect("signing bytes");
    let digest = statement.payload_digest().expect("payload digest");

    assert!(signing_bytes.starts_with(b"crux.execution.authorization.v1\0"));
    assert_eq!(
        digest.value(),
        "465aa618db471a32930ebb88f8d458bf8a65697a8ebf29c5cbfb05a199718a2c"
    );
}

#[test]
fn authorization_statement_golden_fixture_round_trips() {
    let fixture = include_str!("fixtures/authorization-statement-v1.json");
    let statement: AuthorizationStatementV1 =
        serde_json::from_str(fixture).expect("valid authorization fixture");
    let expected: serde_json::Value = serde_json::from_str(fixture).expect("fixture JSON");

    assert_eq!(
        statement.payload_digest().expect("payload digest").value(),
        "465aa618db471a32930ebb88f8d458bf8a65697a8ebf29c5cbfb05a199718a2c"
    );
    assert_eq!(
        serde_json::to_value(statement).expect("serialize statement"),
        expected
    );
    assert_eq!(
        canonicalize_json_v1(fixture.as_bytes()).expect("canonical statement bytes"),
        include_str!("fixtures/authorization-statement-v1.jcs")
            .trim_end()
            .as_bytes()
    );
}

#[test]
fn authorization_statement_rejects_invalid_time_window() {
    let sample = sample_authorization_statement();
    assert!(
        AuthorizationStatementV1::new(
            sample.execution_identity().clone(),
            sample.intent_digest().clone(),
            sample.authority().clone(),
            sample.policy().clone(),
            sample.decision().clone(),
            sample.reasons().to_vec(),
            SignatureAlgorithmV1::Ed25519,
            sample.key_id(),
            sample.audience(),
            fixed_time(),
            fixed_time(),
            AuthorizationNonce::new("nonce-time").expect("nonce"),
        )
        .is_err()
    );
}

#[test]
fn authorization_proof_requires_ed25519_base64url_and_matching_digest() {
    let statement = sample_authorization_statement();
    let payload_digest = statement.payload_digest().expect("payload digest");
    assert!(AuthorizationProofV1::new(payload_digest.clone(), "not base64!").is_err());
    assert!(AuthorizationProofV1::new(payload_digest.clone(), "AAAA").is_err());

    let proof = AuthorizationProofV1::new(payload_digest, "A".repeat(86)).expect("valid proof");
    let record = AuthorizationRecordV1::new(statement.clone(), proof).expect("record");
    assert_eq!(record.statement(), &statement);

    let wrong = AuthorizationProofV1::new(ContentDigestV1::digest(b"wrong"), "A".repeat(86))
        .expect("well-formed wrong proof");
    assert!(AuthorizationRecordV1::new(statement, wrong).is_err());
}

#[test]
fn authorization_statement_wire_round_trip_preserves_exact_times() {
    let statement = sample_authorization_statement();
    let encoded = serde_json::to_string(&statement).expect("serialize statement");
    assert!(encoded.contains("2026-09-22T00:00:00.000000000Z"));
    let decoded: AuthorizationStatementV1 =
        serde_json::from_str(&encoded).expect("deserialize statement");

    assert_eq!(decoded, statement);
}

#[test]
fn verifier_accessors_expose_signed_fields() {
    let statement = sample_authorization_statement();
    let proof = AuthorizationProofV1::new(
        statement.payload_digest().expect("payload digest"),
        "A".repeat(86),
    )
    .expect("proof");
    let envelope = proposed_envelope();
    let expected_intent = sample_intent(json!({
        "image": "alpine",
        "digest": "sha256:abc"
    }));

    assert_eq!(envelope.intent(), Some(&expected_intent));
    assert_eq!(statement.algorithm(), SignatureAlgorithmV1::Ed25519);
    assert_eq!(statement.issued_at(), &fixed_time());
    assert_eq!(
        statement.expires_at(),
        &(fixed_time() + chrono::Duration::minutes(5))
    );
    assert_eq!(statement.nonce().as_str(), "nonce-authorization-1");
    assert_eq!(proof.signature_bytes().expect("signature bytes"), [0; 64]);
}

#[test]
fn retry_decision_uses_exact_timestamp_wire_format() {
    let decision = AuthorizationDecisionV1::RetryLater {
        not_before: fixed_time(),
    };
    let encoded = serde_json::to_string(&decision).expect("serialize retry decision");

    assert!(encoded.contains("2026-09-22T00:00:00.000000000Z"));
    let decoded: AuthorizationDecisionV1 =
        serde_json::from_str(&encoded).expect("deserialize retry decision");
    assert_eq!(decoded, decision);
}

fn sample_policy() -> PolicyIdentityV1 {
    PolicyIdentityV1::new(
        "minibox-local",
        "1.0.0",
        ContentDigestV1::sha256("33".repeat(32)).expect("policy digest"),
    )
    .expect("policy")
}

fn sample_component(name: &str) -> ComponentIdentityV1 {
    ComponentIdentityV1::new(name, "1.0.0", None).expect("component")
}

fn authorization_for(
    envelope: &ExecutionEnvelopeV1,
    identity: ExecutionIdentityV1,
    decision: AuthorizationDecisionV1,
) -> AuthorizationRecordV1 {
    let statement = AuthorizationStatementV1::new(
        identity,
        envelope.records()[0].digest().clone(),
        ActorIdentityV1::new(ActorKindV1::Service, "rulery").expect("authority"),
        sample_policy(),
        decision,
        Vec::new(),
        SignatureAlgorithmV1::Ed25519,
        "rulery-key-1",
        "minibox://native-test",
        fixed_time(),
        fixed_time() + chrono::Duration::minutes(5),
        AuthorizationNonce::new("nonce-envelope").expect("nonce"),
    )
    .expect("statement");
    let proof = AuthorizationProofV1::new(
        statement.payload_digest().expect("payload digest"),
        "A".repeat(86),
    )
    .expect("proof");
    AuthorizationRecordV1::new(statement, proof).expect("authorization record")
}

fn proposed_envelope() -> ExecutionEnvelopeV1 {
    ExecutionEnvelopeV1::new(
        sample_identity(),
        sample_intent(json!({ "image": "alpine", "digest": "sha256:abc" })),
        fixed_time(),
    )
    .expect("proposed envelope")
}

#[test]
fn proposed_envelope_is_a_valid_incomplete_prefix() {
    let envelope = proposed_envelope();

    assert_eq!(
        envelope.lifecycle_state().expect("lifecycle"),
        ExecutionLifecycleV1::Proposed
    );
    assert!(envelope.validate_prefix().is_ok());
    assert!(envelope.validate_complete().is_err());
}

#[test]
fn envelope_rejects_authorization_identity_substitution_and_escalation() {
    let mut envelope = proposed_envelope();
    let substituted_identity = ExecutionIdentityV1::new(
        ExecutionId::new(),
        None,
        Vec::new(),
        ActorIdentityV1::new(ActorKindV1::Agent, "other-agent").expect("actor"),
    )
    .expect("identity");
    let substituted = authorization_for(
        &envelope,
        substituted_identity,
        AuthorizationDecisionV1::Allow {
            grant: empty_capabilities(),
        },
    );
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Authorization(Box::new(substituted)),
                fixed_time() + chrono::Duration::seconds(1),
            )
            .is_err()
    );

    let escalated = CapabilitySetV1::new(
        Vec::new(),
        NetworkCapabilityV1::Unrestricted,
        Vec::new(),
        Vec::new(),
        ProcessCapabilityV1::new(false),
        ResourceLimitsV1::default(),
    )
    .expect("escalated grant");
    let authorization = authorization_for(
        &envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Allow { grant: escalated },
    );
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
                fixed_time() + chrono::Duration::seconds(1),
            )
            .is_err()
    );
}

fn append_allow_and_admission(envelope: &mut ExecutionEnvelopeV1) -> CapabilitySetV1 {
    let grant = empty_capabilities();
    let authorization = authorization_for(
        envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Allow {
            grant: grant.clone(),
        },
    );
    envelope
        .append(
            ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
            fixed_time() + chrono::Duration::seconds(1),
        )
        .expect("authorization");
    envelope
        .append(
            ExecutionRecordPayloadV1::Admission(Box::new(
                AdmissionRecordV1::admitted(
                    sample_component("minibox"),
                    sample_policy(),
                    grant.clone(),
                )
                .expect("admission"),
            )),
            fixed_time() + chrono::Duration::seconds(2),
        )
        .expect("admission");
    grant
}

#[test]
fn successful_envelope_follows_admitted_started_result_path() {
    let mut envelope = proposed_envelope();
    let grant = append_allow_and_admission(&mut envelope);
    envelope
        .append(
            ExecutionRecordPayloadV1::Environment(Box::new(
                ExecutionEnvironmentV1::new(
                    sample_component("minibox"),
                    sample_component("native-linux"),
                    None,
                    grant,
                    Vec::new(),
                )
                .expect("environment"),
            )),
            fixed_time() + chrono::Duration::seconds(3),
        )
        .expect("environment");
    envelope
        .append(
            ExecutionRecordPayloadV1::Started(ExecutionStartedV1::new(
                fixed_time() + chrono::Duration::seconds(4),
            )),
            fixed_time() + chrono::Duration::seconds(4),
        )
        .expect("started");
    envelope
        .append(
            ExecutionRecordPayloadV1::Result(Box::new(
                ExecutionResultV1::new(
                    ExecutionStatusV1::Succeeded,
                    fixed_time() + chrono::Duration::seconds(5),
                    Some(0),
                    Some(json!({ "ok": true })),
                    None,
                    ReplayMetadataV1::new(false, None),
                )
                .expect("result"),
            )),
            fixed_time() + chrono::Duration::seconds(5),
        )
        .expect("result");

    assert_eq!(
        envelope.lifecycle_state().expect("lifecycle"),
        ExecutionLifecycleV1::Completed
    );
    assert_eq!(
        envelope.terminal_status(),
        Some(ExecutionStatusV1::Succeeded)
    );
    assert!(envelope.validate_complete().is_ok());
}

#[test]
fn admission_denial_is_terminal_without_start() {
    let mut envelope = proposed_envelope();
    let authorization = authorization_for(
        &envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Allow {
            grant: empty_capabilities(),
        },
    );
    envelope
        .append(
            ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
            fixed_time() + chrono::Duration::seconds(1),
        )
        .expect("authorization");
    envelope
        .append(
            ExecutionRecordPayloadV1::Admission(Box::new(
                AdmissionRecordV1::denied(
                    sample_component("minibox"),
                    sample_policy(),
                    vec![DecisionReasonV1::new("local.deny", "image unavailable").expect("reason")],
                )
                .expect("denial"),
            )),
            fixed_time() + chrono::Duration::seconds(2),
        )
        .expect("admission denial");

    assert_eq!(
        envelope.lifecycle_state().expect("lifecycle"),
        ExecutionLifecycleV1::AdmissionDenied
    );
    assert_eq!(envelope.terminal_status(), Some(ExecutionStatusV1::Denied));
    assert!(envelope.validate_complete().is_ok());
}

#[test]
fn prestart_failure_is_complete_but_success_before_start_is_rejected() {
    let mut envelope = proposed_envelope();
    append_allow_and_admission(&mut envelope);
    let success = ExecutionResultV1::new(
        ExecutionStatusV1::Succeeded,
        fixed_time() + chrono::Duration::seconds(3),
        Some(0),
        None,
        None,
        ReplayMetadataV1::new(false, None),
    )
    .expect("success result");
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Result(Box::new(success)),
                fixed_time() + chrono::Duration::seconds(3),
            )
            .is_err()
    );

    let failure = ExecutionResultV1::new(
        ExecutionStatusV1::Failed,
        fixed_time() + chrono::Duration::seconds(3),
        None,
        None,
        Some(
            ExecutionErrorV1::new(
                ExecutionFailurePhaseV1::Preparation,
                "rootfs",
                "failed to prepare rootfs",
            )
            .expect("error"),
        ),
        ReplayMetadataV1::new(false, None),
    )
    .expect("failure result");
    envelope
        .append(
            ExecutionRecordPayloadV1::Result(Box::new(failure)),
            fixed_time() + chrono::Duration::seconds(3),
        )
        .expect("prestart failure");
    assert!(envelope.validate_complete().is_ok());
}

#[test]
fn verification_must_reference_matching_terminal_record() {
    let mut envelope = proposed_envelope();
    let authorization = authorization_for(
        &envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Deny,
    );
    let terminal = envelope
        .append(
            ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
            fixed_time() + chrono::Duration::seconds(1),
        )
        .expect("denial")
        .clone();
    let verification = VerificationRecordV1::new(
        sample_component("taskit"),
        terminal.record_id().clone(),
        terminal.digest().clone(),
        VerificationStatusV1::Passed,
        vec!["denial-recorded".into()],
        Vec::<EvidenceRefV1>::new(),
    )
    .expect("verification");
    envelope
        .append(
            ExecutionRecordPayloadV1::Verification(Box::new(verification)),
            fixed_time() + chrono::Duration::seconds(2),
        )
        .expect("verification");

    assert_eq!(
        envelope.lifecycle_state().expect("lifecycle"),
        ExecutionLifecycleV1::Verified
    );
    assert!(envelope.validate_complete().is_ok());
}

#[test]
fn authorization_terminal_decisions_complete_without_execution() {
    let decisions = [
        AuthorizationDecisionV1::Deny,
        AuthorizationDecisionV1::RequireReview {
            review_id: "review-1".into(),
        },
        AuthorizationDecisionV1::RetryLater {
            not_before: fixed_time() + chrono::Duration::minutes(10),
        },
    ];

    for decision in decisions {
        let mut envelope = proposed_envelope();
        let authorization = authorization_for(&envelope, envelope.identity().clone(), decision);
        envelope
            .append(
                ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
                fixed_time() + chrono::Duration::seconds(1),
            )
            .expect("terminal authorization");
        assert!(envelope.validate_complete().is_ok());
    }
}

#[test]
fn envelope_rejects_environment_capability_drift_and_backward_time() {
    let mut envelope = proposed_envelope();
    append_allow_and_admission(&mut envelope);
    let drifted = CapabilitySetV1::new(
        Vec::new(),
        NetworkCapabilityV1::Unrestricted,
        Vec::new(),
        Vec::new(),
        ProcessCapabilityV1::new(false),
        ResourceLimitsV1::default(),
    )
    .expect("drifted capabilities");
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Environment(Box::new(
                    ExecutionEnvironmentV1::new(
                        sample_component("minibox"),
                        sample_component("native-linux"),
                        None,
                        drifted,
                        Vec::new(),
                    )
                    .expect("environment"),
                )),
                fixed_time() + chrono::Duration::seconds(3),
            )
            .is_err()
    );
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Environment(Box::new(
                    ExecutionEnvironmentV1::new(
                        sample_component("minibox"),
                        sample_component("native-linux"),
                        None,
                        empty_capabilities(),
                        Vec::new(),
                    )
                    .expect("environment"),
                )),
                fixed_time(),
            )
            .is_err()
    );
}

#[test]
fn v1_constructor_limits_are_enforced() {
    let execution = ExecutionId::from_str(&format!("exec_{ULID}")).expect("execution ID");
    let parents = (0_u32..17)
        .map(|index| {
            ExecutionId::from_str(&format!("exec_{}", Ulid::from(u128::from(index + 1))))
                .expect("parent ID")
        })
        .collect();
    assert!(
        ExecutionIdentityV1::new(
            execution,
            None,
            parents,
            ActorIdentityV1::new(ActorKindV1::Agent, "looprs").expect("actor"),
        )
        .is_err()
    );

    let scopes = (0..65)
        .map(|index| {
            FilesystemScopeV1::new(format!("/scope/{index:02}"), FilesystemAccessV1::Read)
                .expect("scope")
        })
        .collect();
    assert!(
        CapabilitySetV1::new(
            scopes,
            NetworkCapabilityV1::Denied,
            Vec::new(),
            Vec::new(),
            ProcessCapabilityV1::new(false),
            ResourceLimitsV1::default(),
        )
        .is_err()
    );

    let endpoints = (0..65)
        .map(|index| {
            NetworkEndpointV1::new("https", format!("host-{index:02}.example"), None)
                .expect("endpoint")
        })
        .collect();
    assert!(NetworkCapabilityV1::allow_list(endpoints).is_err());

    let implicit_https = NetworkEndpointV1::new("https", "example.com", None).expect("endpoint");
    let explicit_https =
        NetworkEndpointV1::new("https", "example.com", Some(443)).expect("endpoint");
    let implicit = NetworkCapabilityV1::allow_list(vec![implicit_https]).expect("allowlist");
    let explicit = NetworkCapabilityV1::allow_list(vec![explicit_https]).expect("allowlist");
    assert!(!implicit.contains(&explicit));
    assert!(!explicit.contains(&implicit));

    assert!(
        ExecutionResultV1::new(
            ExecutionStatusV1::Failed,
            fixed_time(),
            None,
            None,
            None,
            ReplayMetadataV1::new(false, None),
        )
        .is_err()
    );
    assert!(
        ExecutionResultV1::new(
            ExecutionStatusV1::Succeeded,
            fixed_time(),
            Some(0),
            Some(json!({ "invalid": -1 })),
            None,
            ReplayMetadataV1::new(false, None),
        )
        .is_err()
    );
}

#[test]
fn verification_rejects_mismatched_terminal_digest() {
    let mut envelope = proposed_envelope();
    let authorization = authorization_for(
        &envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Deny,
    );
    let terminal = envelope
        .append(
            ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
            fixed_time() + chrono::Duration::seconds(1),
        )
        .expect("denial")
        .clone();
    let verification = VerificationRecordV1::new(
        sample_component("taskit"),
        terminal.record_id().clone(),
        ContentDigestV1::digest(b"wrong terminal"),
        VerificationStatusV1::Passed,
        vec!["denial-recorded".into()],
        Vec::new(),
    )
    .expect("verification");

    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Verification(Box::new(verification)),
                fixed_time() + chrono::Duration::seconds(2),
            )
            .is_err()
    );
}

#[test]
fn started_requires_environment_and_matching_executor() {
    let mut envelope = proposed_envelope();
    let grant = append_allow_and_admission(&mut envelope);
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Started(ExecutionStartedV1::new(
                    fixed_time() + chrono::Duration::seconds(3),
                )),
                fixed_time() + chrono::Duration::seconds(3),
            )
            .is_err()
    );
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Environment(Box::new(
                    ExecutionEnvironmentV1::new(
                        sample_component("different-executor"),
                        sample_component("native-linux"),
                        None,
                        grant,
                        Vec::new(),
                    )
                    .expect("environment"),
                )),
                fixed_time() + chrono::Duration::seconds(3),
            )
            .is_err()
    );
}

#[test]
fn cpu_weight_requires_exact_optional_value() {
    let unrequested = ResourceLimitsV1::new(None, None, None, None, None).expect("resources");
    let introduced = ResourceLimitsV1::new(None, None, Some(100), None, None).expect("resources");

    assert!(!unrequested.contains(&introduced));
    assert!(!introduced.contains(&unrequested));
}

#[test]
fn action_wire_rejects_nested_duplicate_keys() {
    let wire = r#"{
        "namespace":"minibox.container",
        "name":"run.v1",
        "input":{"nested":{"a":1,"a":2}},
        "input_schema":null,
        "side_effect":"reversible",
        "idempotency_key":null
    }"#;

    assert!(serde_json::from_str::<ActionSpecV1>(wire).is_err());
}

#[test]
fn envelope_raw_parser_rejects_duplicate_action_keys() {
    let encoded = serde_json::to_string(&proposed_envelope()).expect("serialize envelope");
    let duplicate = encoded.replacen(
        "\"image\":\"alpine\"",
        "\"image\":\"alpine\",\"image\":\"evil\"",
        1,
    );

    assert!(ExecutionEnvelopeV1::from_json_bytes(duplicate.as_bytes()).is_err());
}

#[test]
fn non_success_results_require_classified_failure_phase() {
    for status in [
        ExecutionStatusV1::Failed,
        ExecutionStatusV1::Cancelled,
        ExecutionStatusV1::TimedOut,
    ] {
        assert!(
            ExecutionResultV1::new(
                status,
                fixed_time(),
                None,
                None,
                None,
                ReplayMetadataV1::new(false, None),
            )
            .is_err()
        );
    }
}

#[test]
fn record_construction_revalidates_public_leaf_fields() {
    let mut intent = sample_intent(json!({ "ok": true }));
    intent.action.namespace.clear();
    assert!(
        ExecutionRecordV1::new(
            ExecutionRecordId::new(),
            0,
            fixed_time(),
            None,
            ExecutionRecordPayloadV1::Intent(Box::new(intent)),
        )
        .is_err()
    );

    let mut verification = VerificationRecordV1::new(
        sample_component("taskit"),
        ExecutionRecordId::new(),
        ContentDigestV1::digest(b"subject"),
        VerificationStatusV1::Passed,
        vec!["check".into()],
        Vec::new(),
    )
    .expect("verification");
    verification.checks.push("check".into());
    assert!(
        ExecutionRecordV1::new(
            ExecutionRecordId::new(),
            1,
            fixed_time(),
            Some(ContentDigestV1::digest(b"previous")),
            ExecutionRecordPayloadV1::Verification(Box::new(verification)),
        )
        .is_err()
    );
}

#[test]
fn review_and_retry_records_cannot_be_verified() {
    for decision in [
        AuthorizationDecisionV1::RequireReview {
            review_id: "review-1".into(),
        },
        AuthorizationDecisionV1::RetryLater {
            not_before: fixed_time() + chrono::Duration::minutes(1),
        },
    ] {
        let mut envelope = proposed_envelope();
        let authorization = authorization_for(&envelope, envelope.identity().clone(), decision);
        let subject = envelope
            .append(
                ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
                fixed_time() + chrono::Duration::seconds(1),
            )
            .expect("terminal policy record")
            .clone();
        let verification = VerificationRecordV1::new(
            sample_component("taskit"),
            subject.record_id().clone(),
            subject.digest().clone(),
            VerificationStatusV1::Passed,
            vec!["policy-recorded".into()],
            Vec::new(),
        )
        .expect("verification");
        assert!(
            envelope
                .append(
                    ExecutionRecordPayloadV1::Verification(Box::new(verification)),
                    fixed_time() + chrono::Duration::seconds(2),
                )
                .is_err()
        );
    }
}

#[test]
fn started_timestamp_cannot_predate_environment_record() {
    let mut envelope = proposed_envelope();
    let grant = append_allow_and_admission(&mut envelope);
    envelope
        .append(
            ExecutionRecordPayloadV1::Environment(Box::new(
                ExecutionEnvironmentV1::new(
                    sample_component("minibox"),
                    sample_component("native-linux"),
                    None,
                    grant,
                    Vec::new(),
                )
                .expect("environment"),
            )),
            fixed_time() + chrono::Duration::seconds(3),
        )
        .expect("environment");

    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Started(ExecutionStartedV1::new(
                    fixed_time() + chrono::Duration::seconds(2),
                )),
                fixed_time() + chrono::Duration::seconds(4),
            )
            .is_err()
    );
}

#[test]
fn cumulative_output_must_fit_effective_limit_and_media_contract() {
    let capabilities = CapabilitySetV1::new(
        Vec::new(),
        NetworkCapabilityV1::Denied,
        Vec::new(),
        Vec::new(),
        ProcessCapabilityV1::new(false),
        ResourceLimitsV1::new(None, None, None, None, Some(10)).expect("resources"),
    )
    .expect("capabilities");
    let intent = ExecutionIntentV1 {
        action: ActionSpecV1::new(
            "minibox.container",
            "run.v1",
            json!({ "image": "alpine" }),
            None,
            SideEffectClassV1::Reversible,
            None,
        )
        .expect("action"),
        requested_capabilities: capabilities.clone(),
        expected_output: OutputContractV1::new(None, vec!["text/plain".into()], 10)
            .expect("output contract"),
    };
    let mut envelope =
        ExecutionEnvelopeV1::new(sample_identity(), intent, fixed_time()).expect("envelope");
    let authorization = authorization_for(
        &envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Allow {
            grant: capabilities.clone(),
        },
    );
    envelope
        .append(
            ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
            fixed_time() + chrono::Duration::seconds(1),
        )
        .expect("authorization");
    envelope
        .append(
            ExecutionRecordPayloadV1::Admission(Box::new(
                AdmissionRecordV1::admitted(
                    sample_component("minibox"),
                    sample_policy(),
                    capabilities.clone(),
                )
                .expect("admission"),
            )),
            fixed_time() + chrono::Duration::seconds(2),
        )
        .expect("admission");
    envelope
        .append(
            ExecutionRecordPayloadV1::Environment(Box::new(
                ExecutionEnvironmentV1::new(
                    sample_component("minibox"),
                    sample_component("native-linux"),
                    None,
                    capabilities,
                    Vec::new(),
                )
                .expect("environment"),
            )),
            fixed_time() + chrono::Duration::seconds(3),
        )
        .expect("environment");
    envelope
        .append(
            ExecutionRecordPayloadV1::Started(ExecutionStartedV1::new(
                fixed_time() + chrono::Duration::seconds(4),
            )),
            fixed_time() + chrono::Duration::seconds(4),
        )
        .expect("started");
    let output = || {
        ExecutionOutputV1::new(
            OutputStreamV1::Stdout,
            EvidenceRefV1::new("text/plain", ContentDigestV1::digest(b"output"), 6, None)
                .expect("evidence"),
            6,
            6,
            false,
        )
        .expect("output")
    };
    envelope
        .append(
            ExecutionRecordPayloadV1::Output(Box::new(output())),
            fixed_time() + chrono::Duration::seconds(5),
        )
        .expect("first output");
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Output(Box::new(output())),
                fixed_time() + chrono::Duration::seconds(6),
            )
            .is_err()
    );
}

#[test]
fn envelope_rejects_sequence_gap_and_broken_predecessor_digest() {
    let mut envelope = proposed_envelope();
    let authorization = authorization_for(
        &envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Allow {
            grant: empty_capabilities(),
        },
    );
    envelope
        .append(
            ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
            fixed_time() + chrono::Duration::seconds(1),
        )
        .expect("authorization");
    let original = serde_json::to_value(&envelope).expect("envelope value");

    let mut gap = original.clone();
    gap["records"][1]["sequence"] = json!(3);
    assert!(
        ExecutionEnvelopeV1::from_json_bytes(
            &serde_json::to_vec(&gap).expect("serialize sequence gap"),
        )
        .is_err()
    );

    let mut broken = original;
    broken["records"][1]["previous_digest"]["value"] = json!("00".repeat(32));
    assert!(
        ExecutionEnvelopeV1::from_json_bytes(
            &serde_json::to_vec(&broken).expect("serialize broken chain"),
        )
        .is_err()
    );
}

#[test]
fn terminal_envelope_rejects_duplicate_terminal_record() {
    let mut envelope = proposed_envelope();
    let denial = authorization_for(
        &envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Deny,
    );
    envelope
        .append(
            ExecutionRecordPayloadV1::Authorization(Box::new(denial)),
            fixed_time() + chrono::Duration::seconds(1),
        )
        .expect("denial");
    let duplicate = authorization_for(
        &envelope,
        envelope.identity().clone(),
        AuthorizationDecisionV1::Deny,
    );
    assert!(
        envelope
            .append(
                ExecutionRecordPayloadV1::Authorization(Box::new(duplicate)),
                fixed_time() + chrono::Duration::seconds(2),
            )
            .is_err()
    );
}

#[test]
fn authorization_binds_each_execution_identity_dimension() {
    let base = sample_identity();
    let parent = ExecutionId::from_str("exec_01ARZ3NDEKTSV4RRFFQ69G5FAW").expect("parent");
    let substitutions = [
        ExecutionIdentityV1::new(
            ExecutionId::new(),
            base.session_id().cloned(),
            base.parent_execution_ids().to_vec(),
            base.actor().clone(),
        )
        .expect("execution substitution"),
        ExecutionIdentityV1::new(
            base.execution_id().clone(),
            Some(CruxId::new()),
            base.parent_execution_ids().to_vec(),
            base.actor().clone(),
        )
        .expect("session substitution"),
        ExecutionIdentityV1::new(
            base.execution_id().clone(),
            base.session_id().cloned(),
            vec![parent],
            base.actor().clone(),
        )
        .expect("parent substitution"),
        ExecutionIdentityV1::new(
            base.execution_id().clone(),
            base.session_id().cloned(),
            base.parent_execution_ids().to_vec(),
            ActorIdentityV1::new(ActorKindV1::Agent, "other-agent").expect("actor"),
        )
        .expect("actor substitution"),
    ];

    for substitution in substitutions {
        let mut envelope = proposed_envelope();
        let authorization = authorization_for(
            &envelope,
            substitution,
            AuthorizationDecisionV1::Allow {
                grant: empty_capabilities(),
            },
        );
        assert!(
            envelope
                .append(
                    ExecutionRecordPayloadV1::Authorization(Box::new(authorization)),
                    fixed_time() + chrono::Duration::seconds(1),
                )
                .is_err()
        );
    }
}
