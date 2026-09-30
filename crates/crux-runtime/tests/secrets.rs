use std::collections::HashMap;

use crux_runtime::prelude::*;
use crux_runtime::secrets::{
    SecretRedactor, SecretRef, SecretResolver, SecretValue, resolve_secret_refs,
};
use serde_json::json;

struct MapResolver(HashMap<String, String>);

impl SecretResolver for MapResolver {
    fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, CruxErr> {
        self.0
            .get(reference.key())
            .cloned()
            .map(SecretValue::new)
            .ok_or_else(|| CruxErr::step_failed("secret", "missing secret"))
    }
}

#[tokio::test]
async fn secret_references_resolve_without_leaking_into_serialized_traces() {
    let resolver = MapResolver(HashMap::from([(
        "api-key".to_owned(),
        "super-secret-value".to_owned(),
    )]));
    let mut redactor = SecretRedactor::new();
    let resolved = resolve_secret_refs(
        json!({"token": {"$secret": "api-key", "provider": "test"}}),
        &resolver,
        &mut redactor,
    )
    .unwrap();
    assert_eq!(resolved["token"], "super-secret-value");

    let mut ctx = CruxCtx::new("secret-test");
    ctx.set_redactor(Box::new(redactor));
    ctx.step("uses-secret", || async {
        Ok::<_, CruxErr>(resolved.clone())
    })
    .await
    .unwrap();
    let trace = ctx.finalize(Ok(json!("done")));
    let serialized = serde_json::to_string(&trace).unwrap();

    assert!(!serialized.contains("super-secret-value"));
    assert!(serialized.contains("***"));
}

#[test]
fn secret_values_are_redacted_in_debug_output() {
    let value = SecretValue::new("sensitive");
    assert_eq!(format!("{value:?}"), "SecretValue(***REDACTED***)");
}
