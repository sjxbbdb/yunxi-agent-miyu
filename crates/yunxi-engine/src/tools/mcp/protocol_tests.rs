use super::protocol::{classify, Incoming};

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
