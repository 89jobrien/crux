//! JSON round-trip tests for plugin protocol requests and responses.

use crux_plugin::protocol::{
    HandlerDecl, InvocationId, PROTOCOL_VERSION, ProtocolVersion, Request, Response, StreamEvent,
};

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
        handlers: vec![HandlerDecl {
            name: "github::create_issue".into(),
            description: "Create a GitHub issue".into(),
        }],
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
