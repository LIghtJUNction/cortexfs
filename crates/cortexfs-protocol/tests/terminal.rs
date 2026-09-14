use cortexfs_protocol::{EventStatus, ModelEvent, WireProtocol, decode_response_events};

#[test]
fn terminal_failure_suppresses_tool_calls() {
    let cases: [(WireProtocol, &[u8]); 2] = [
        (WireProtocol::Anthropic, br#"{"id":"r","model":"m","content":[{"type":"tool_use","id":"call_1","name":"tsh","input":{"args":["tools"]}}],"stop_reason":"error"}"#),
        (WireProtocol::OpenAiChat, br#"{"id":"r","model":"m","choices":[{"message":{"content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"tsh","arguments":"{\"args\":[\"tools\"]}"}}]},"finish_reason":"cancelled"}]}"#),
    ];
    for (protocol, body) in cases {
        let events = decode_response_events(protocol, body).expect("decode terminal response");
        assert!(!events.iter().any(|event| matches!(event, ModelEvent::ToolCall { .. })));
        assert!(events.iter().any(|event| matches!(event, ModelEvent::Done { status: EventStatus::Error | EventStatus::Cancelled, .. })));
    }
}
