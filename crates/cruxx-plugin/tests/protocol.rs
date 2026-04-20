use cruxx_plugin::protocol::{HandlerDecl, Request, Response};

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
