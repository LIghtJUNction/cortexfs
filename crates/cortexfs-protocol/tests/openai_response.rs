use cortexfs_protocol::{ConversionError, WireProtocol, decode_response_events};

#[test]
fn openai_chat_rejects_empty_choices() {
    let input = br#"{"id":"run","model":"model","choices":[]}"#;
    assert!(matches!(
        decode_response_events(WireProtocol::OpenAiChat, input),
        Err(ConversionError::InvalidField { field, .. }) if field == "choices"
    ));
}
