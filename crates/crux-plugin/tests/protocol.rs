//! JSON round-trip tests for plugin protocol requests and responses.

use crux_plugin::protocol::{
    HandlerDecl, InvocationId, PROTOCOL_VERSION, ProtocolVersion, Request, Response, StreamEvent,
};
use crux_script::{ObjectSchema, ValueSchema};

#[test]
fn declare_request_round_trips() {
    let req = Request::Declare;
    let json = serde_json::to_string(&req).unwrap();
    let back: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, Request::Declare));
}

#[test]
fn invoke_request_round_trips() {
    let req = Request::Invoke {
        handler: "github::create_issue".into(),
        input: serde_json::json!({"title": "test"}),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: Request = serde_json::from_str(&json).unwrap();
    match back {
        Request::Invoke { handler, input } => {
            assert_eq!(handler, "github::create_issue");
            assert_eq!(input["title"], "test");
        }
        _ => panic!("expected Invoke"),
    }
}

#[test]
fn declare_response_round_trips() {
    let resp = Response::Declare {
        handlers: vec![HandlerDecl::new(
            "github::create_issue",
            "Create a GitHub issue",
        )],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&json).unwrap();
    match back {
        Response::Declare { handlers } => {
            assert_eq!(handlers.len(), 1);
            assert_eq!(handlers[0].name, "github::create_issue");
        }
        _ => panic!("expected Declare"),
    }
}

#[test]
fn declared_output_schema_round_trips() {
    let schema = ValueSchema::array(ValueSchema::Object(
        ObjectSchema::new().required("id", ValueSchema::String),
    ));
    let resp = Response::Declare {
        handlers: vec![
            HandlerDecl::new("github::list_issues", "List issues").output_schema(schema.clone()),
        ],
    };

    let back: Response = serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
    let Response::Declare { handlers } = back else {
        panic!("expected Declare");
    };
    assert_eq!(handlers[0].output_schema.as_ref(), Some(&schema));
}

/// Pins the object-schema payload shown in `docs/crux-plugins.md`, so the
/// documented `definition` nesting stays something a real plugin can send.
#[test]
fn documented_object_schema_example_deserializes() {
    let json = r#"{
        "type": "array",
        "definition": {
            "items": {
                "type": "object",
                "definition": {
                    "properties": {
                        "name": { "schema": { "type": "string" }, "required": true }
                    }
                }
            }
        }
    }"#;

    serde_json::from_str::<ValueSchema>(json)
        .expect("the object schema shown in docs/crux-plugins.md must load");
}

/// A plugin built against protocol 1.0 sends no `output_schema` key. The field
/// is additive, so such a plugin must still load.
#[test]
fn legacy_declaration_without_output_schema_still_deserializes() {
    let legacy = r#"{
        "status": "Declare",
        "data": {
            "handlers": [
                { "name": "echo::reflect", "description": "Returns input unchanged" }
            ]
        }
    }"#;

    let resp: Response = serde_json::from_str(legacy).unwrap();
    let Response::Declare { handlers } = resp else {
        panic!("expected Declare");
    };
    assert_eq!(handlers[0].name, "echo::reflect");
    assert_eq!(handlers[0].output_schema, None);
}

/// A nested schema must arrive structurally intact, not flattened to `Dynamic`.
///
/// The expected JSON is spelled out so the wire shape documented in
/// `docs/crux-plugins.md` cannot drift from what plugin authors must send.
#[test]
fn nested_declared_schema_survives_the_wire() {
    let schema = ValueSchema::array(ValueSchema::Object(ObjectSchema::new()));
    assert_eq!(
        serde_json::to_string(&schema).unwrap(),
        r#"{"type":"array","definition":{"items":{"type":"object","definition":{"properties":{},"additional":null}}}}"#
    );

    let mut handlers = serde_json::json!({
        "name": "echo::lines",
        "description": "Split input into lines",
    });
    handlers["output_schema"] = serde_json::to_value(&schema).unwrap();
    let json = serde_json::json!({"status": "Declare", "data": {"handlers": [handlers]}});

    let resp: Response = serde_json::from_value(json).unwrap();
    let Response::Declare { handlers } = resp else {
        panic!("expected Declare");
    };
    assert_eq!(handlers[0].output_schema.as_ref(), Some(&schema));
}

#[test]
fn invoke_ok_response_round_trips() {
    let resp = Response::InvokeOk {
        output: serde_json::json!({"id": 42}),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, Response::InvokeOk { .. }));
}

#[test]
fn invoke_err_response_round_trips() {
    let resp = Response::InvokeErr {
        error: "not found".into(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&json).unwrap();
    match back {
        Response::InvokeErr { error } => assert_eq!(error, "not found"),
        _ => panic!("expected InvokeErr"),
    }
}

#[test]
fn shutdown_request_round_trips() {
    let req = Request::Shutdown;
    let json = serde_json::to_string(&req).unwrap();
    let back: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, Request::Shutdown));
}

#[test]
fn versioned_handshake_round_trips() {
    let request = Request::Handshake {
        version: PROTOCOL_VERSION,
    };
    let json = serde_json::to_string(&request).unwrap();
    let back: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        back,
        Request::Handshake {
            version: ProtocolVersion { major: 1, .. }
        }
    ));
}

#[test]
fn correlated_stream_and_cancellation_messages_round_trip() {
    let id = InvocationId::new("call-42");
    let invoke = Request::InvokeV1 {
        id: id.clone(),
        handler: "github::create_issue".into(),
        input: serde_json::json!({"title": "test"}),
        deadline_ms: Some(5000),
    };
    let cancel = Request::Cancel { id: id.clone() };
    let event = Response::Event {
        id: id.clone(),
        event: StreamEvent::Chunk {
            value: serde_json::json!("partial"),
        },
    };

    for value in [
        serde_json::to_value(invoke).unwrap(),
        serde_json::to_value(cancel).unwrap(),
    ] {
        serde_json::from_value::<Request>(value).unwrap();
    }
    let back: Response = serde_json::from_value(serde_json::to_value(event).unwrap()).unwrap();
    assert!(matches!(back, Response::Event { id: back_id, .. } if back_id == id));
}
