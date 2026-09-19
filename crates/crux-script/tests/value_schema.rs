use crux_script::{SchemaViolationKind, ValueKind, ValueSchema};
use serde_json::{Value, json};

#[test]
fn scalar_schema_validation() {
    let valid_cases = [
        (ValueSchema::Null, Value::Null),
        (ValueSchema::Boolean, json!(true)),
        (ValueSchema::Integer, json!(42)),
        (ValueSchema::Number, json!(42)),
        (ValueSchema::Number, json!(4.2)),
        (ValueSchema::String, json!("crux")),
    ];

    for (schema, value) in valid_cases {
        assert_eq!(schema.validate(&value), Ok(()));
    }

    assert!(ValueSchema::Number.is_assignable_from(&ValueSchema::Integer));
    assert!(!ValueSchema::Integer.is_assignable_from(&ValueSchema::Number));

    let violation = ValueSchema::String.validate(&json!(true)).unwrap_err();
    assert_eq!(violation.path, "$".to_string());
    assert_eq!(violation.expected, ValueSchema::String);
    assert_eq!(
        violation.kind,
        SchemaViolationKind::TypeMismatch {
            actual: ValueKind::Boolean,
        }
    );
}
