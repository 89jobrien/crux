//! Assignability, violation, union, and contract tests for value schemas.

use crux_script::{
    ArgSchema, ConfidenceCapability, HandlerMetadata, ObjectSchema, SchemaBuildError,
    SchemaViolationKind, ValueKind, ValueSchema,
};
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

#[test]
fn object_schema_assignability() {
    let target = ValueSchema::object(
        ObjectSchema::new()
            .required("id", ValueSchema::String)
            .optional("count", ValueSchema::Number),
    );
    let compatible = ValueSchema::object(
        ObjectSchema::new()
            .required("id", ValueSchema::String)
            .optional("count", ValueSchema::Integer),
    );
    let missing_required = ValueSchema::object(ObjectSchema::new());
    let optional_required =
        ValueSchema::object(ObjectSchema::new().optional("id", ValueSchema::String));
    let extra_property = ValueSchema::object(
        ObjectSchema::new()
            .required("id", ValueSchema::String)
            .optional("extra", ValueSchema::String),
    );

    assert!(target.is_assignable_from(&compatible));
    assert!(!target.is_assignable_from(&missing_required));
    assert!(!target.is_assignable_from(&optional_required));
    assert!(!target.is_assignable_from(&extra_property));

    let open_target = ValueSchema::object(ObjectSchema::new().additional(ValueSchema::String));
    assert!(open_target.is_assignable_from(&ValueSchema::object(
        ObjectSchema::new().required("label", ValueSchema::String)
    )));
    assert!(!open_target.is_assignable_from(&ValueSchema::object(
        ObjectSchema::new().required("enabled", ValueSchema::Boolean)
    )));

    let optional_string = ValueSchema::object(
        ObjectSchema::new()
            .optional("label", ValueSchema::String)
            .additional(ValueSchema::Dynamic),
    );
    let dynamic_boolean_property =
        ValueSchema::object(ObjectSchema::new().additional(ValueSchema::Boolean));
    assert!(!optional_string.is_assignable_from(&dynamic_boolean_property));
}

#[test]
fn object_schema_reports_property_violations() {
    let schema = ValueSchema::object(
        ObjectSchema::new()
            .required("id", ValueSchema::String)
            .optional("count", ValueSchema::Integer),
    );

    assert_eq!(schema.validate(&json!({"id": "task", "count": 2})), Ok(()));

    let missing = schema.validate(&json!({})).unwrap_err();
    assert_eq!(missing.path, "$.id");
    assert_eq!(missing.kind, SchemaViolationKind::MissingRequiredProperty);

    let wrong_type = schema.validate(&json!({"id": true})).unwrap_err();
    assert_eq!(wrong_type.path, "$.id");
    assert_eq!(
        wrong_type.kind,
        SchemaViolationKind::TypeMismatch {
            actual: ValueKind::Boolean,
        }
    );

    let additional = schema
        .validate(&json!({"id": "task", "extra": true}))
        .unwrap_err();
    assert_eq!(additional.path, "$.extra");
    assert_eq!(
        additional.kind,
        SchemaViolationKind::AdditionalPropertyNotAllowed
    );
}

#[test]
fn array_schema_is_covariant() {
    let numbers = ValueSchema::array(ValueSchema::Number);
    let integers = ValueSchema::array(ValueSchema::Integer);

    assert!(numbers.is_assignable_from(&integers));
    assert!(!integers.is_assignable_from(&numbers));
    assert_eq!(numbers.validate(&json!([1, 2.5, 3])), Ok(()));

    let violation = integers.validate(&json!([1, true])).unwrap_err();
    assert_eq!(violation.path, "$[1]");
    assert_eq!(
        violation.kind,
        SchemaViolationKind::TypeMismatch {
            actual: ValueKind::Boolean,
        }
    );
}

#[test]
fn union_schema_normalization() {
    let numeric = ValueSchema::union([ValueSchema::Integer, ValueSchema::Number]).unwrap();
    let normalized = ValueSchema::union([
        ValueSchema::String,
        numeric,
        ValueSchema::String,
        ValueSchema::Integer,
    ])
    .unwrap();

    assert_eq!(
        normalized,
        ValueSchema::Union {
            variants: vec![
                ValueSchema::String,
                ValueSchema::Integer,
                ValueSchema::Number,
            ],
        }
    );
    assert_eq!(
        ValueSchema::union([ValueSchema::Boolean]).unwrap(),
        ValueSchema::Boolean
    );
    assert_eq!(normalized.validate(&json!("value")), Ok(()));
    assert_eq!(normalized.validate(&json!(3.5)), Ok(()));

    let deeply_nested = ValueSchema::union([ValueSchema::Union {
        variants: vec![
            ValueSchema::Union {
                variants: vec![ValueSchema::Null, ValueSchema::Boolean],
            },
            ValueSchema::String,
        ],
    }])
    .unwrap();
    assert_eq!(
        deeply_nested,
        ValueSchema::Union {
            variants: vec![ValueSchema::Null, ValueSchema::Boolean, ValueSchema::String,],
        }
    );
}

#[test]
fn empty_union_is_rejected() {
    assert_eq!(ValueSchema::union([]), Err(SchemaBuildError::EmptyUnion));
    assert_eq!(
        ValueSchema::Union {
            variants: Vec::new(),
        }
        .validate_definition(),
        Err(SchemaBuildError::EmptyUnion)
    );
}

#[test]
fn handler_contract_completeness() {
    assert!(!HandlerMetadata::new("test::dynamic").has_complete_contract());

    let metadata = HandlerMetadata::new("test::complete")
        .args(ArgSchema::strict().required(
            "config",
            ValueSchema::object(ObjectSchema::new().required("name", ValueSchema::String)),
        ))
        .input_schema(ValueSchema::Dynamic)
        .output_schema(ValueSchema::String)
        .confidence(ConfidenceCapability::Never);

    assert!(metadata.has_complete_contract());
    assert_eq!(
        metadata.args.get("config").unwrap().schema,
        ValueSchema::object(ObjectSchema::new().required("name", ValueSchema::String))
    );
}
