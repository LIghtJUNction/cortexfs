use cortexfs_protocol::{WireProtocol, decode_response_events};

#[test]
fn openai_chat_rejects_empty_choices() {
    let input = br#"{\"id\":\"run\",\"model\":\"model\",\"choices\":[]}"#;
    assert!(decode_response_events(WireProtocol::OpenAiChat, input).is_err());
}
