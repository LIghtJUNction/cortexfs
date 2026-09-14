use cortexfs_protocol::{ModelEvent, WireProtocol, decode_response_events};

#[test]
fn failed_anthropic_turn_suppresses_tool_calls() {
    let body = br#"{"content":[{"type":"tool_use","id":"c","name":"tsh"}],"stop_reason":"error"}"#;
    let events = decode_response_events(WireProtocol::Anthropic, body).expect("decode terminal response");
    assert!(!events.iter().any(|event| matches!(event, ModelEvent::ToolCall { .. })));
}
