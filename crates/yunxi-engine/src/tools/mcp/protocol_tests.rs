use super::protocol::{
    classify, initialize_params, notification_line, request_line, Incoming, JSONRPC_VERSION,
    PROTOCOL_VERSION,
};
use serde_json::json;

#[test]
fn initialize_request_has_the_expected_wire_shape() {
    let request = serde_json::from_str::<serde_json::Value>(&request_line(
        7,
        "initialize",
        initialize_params(),
    ))
    .unwrap();

    assert_eq!(
        request,
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": 7,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "yunxi", "version": env!("CARGO_PKG_VERSION")}
            }
        })
    );
}

#[test]
fn tools_list_request_has_empty_params() {
    let request =
        serde_json::from_str::<serde_json::Value>(&request_line(8, "tools/list", json!({})))
            .unwrap();

    assert_eq!(request["jsonrpc"], JSONRPC_VERSION);
    assert_eq!(request["id"], 8);
    assert_eq!(request["method"], "tools/list");
    assert_eq!(request["params"], json!({}));
}

#[test]
fn tools_call_request_contains_name_and_arguments() {
    let request = serde_json::from_str::<serde_json::Value>(&request_line(
        9,
        "tools/call",
        json!({
            "name": "echo",
            "arguments": {"text": "hello"}
        }),
    ))
    .unwrap();

    assert_eq!(request["jsonrpc"], JSONRPC_VERSION);
    assert_eq!(request["id"], 9);
    assert_eq!(request["method"], "tools/call");
    assert_eq!(request["params"]["name"], "echo");
    assert_eq!(request["params"]["arguments"], json!({"text": "hello"}));
}

#[test]
fn notification_has_no_id() {
    let notification = serde_json::from_str::<serde_json::Value>(&notification_line(
        "notifications/initialized",
        json!({}),
    ))
    .unwrap();

    assert_eq!(notification["jsonrpc"], JSONRPC_VERSION);
    assert_eq!(notification["method"], "notifications/initialized");
    assert_eq!(notification["params"], json!({}));
    assert!(notification.get("id").is_none());
}

#[test]
fn malformed_responses_are_classified_for_fast_failure() {
    let cases = [
        (
            r#"{"id":7,"result":{}}"#,
            Some(7),
            "missing or invalid jsonrpc version",
        ),
        (
            r#"{"jsonrpc":"2.0","id":"seven","result":{}}"#,
            None,
            "response id is not an unsigned integer",
        ),
        (
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":"bad","message":"nope"}}"#,
            Some(7),
            "response error object is invalid",
        ),
        (
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","result":{}}"#,
            Some(7),
            "response contains method and result/error",
        ),
    ];

    for (line, expected_id, expected_reason) in cases {
        match classify(line) {
            Some(Incoming::InvalidResponse { id, reason }) => {
                assert_eq!(id, expected_id);
                assert_eq!(reason, expected_reason);
            }
            other => panic!("expected invalid response, got {other:?}"),
        }
    }
}

#[test]
fn valid_result_and_error_responses_remain_distinct() {
    assert!(matches!(
        classify(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#),
        Some(Incoming::Response {
            id: 1,
            outcome: Ok(_)
        })
    ));
    assert!(matches!(
        classify(r#"{"jsonrpc":"2.0","id":2,"error":{"code":-1,"message":"nope"}}"#),
        Some(Incoming::Response {
            id: 2,
            outcome: Err(_)
        })
    ));
}
