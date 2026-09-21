use cortexfs_protocol::{
    WireProtocol as W, decode_model_request, decode_response_events, encode_model_request,
};
#[test]
fn malformed_anthropic_tool_use_is_rejected() {
    assert!(decode_response_events(W::Anthropic, br#"{"content":[{"type":"tool_use","id":0}]}"#).is_err());
}
#[test]
fn openai_image_request_roundtrips() -> Result<(), cortexfs_protocol::ConversionError> {
    let input = br#"{"model":"m","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.invalid/i.png"}}]}]}"#;
    let request = decode_model_request(W::OpenAiChat, input)?;
    let encoded = encode_model_request(W::OpenAiChat, &request)?;
    assert_eq!(decode_model_request(W::OpenAiChat, &encoded)?, request);
    Ok(())
}
#[test]
fn openai_rejects_incomplete_responses() {
    for input in [
        br#"{"choices":[]}"#.as_slice(),
        br#"{"choices":[{}]}"#.as_slice(),
        br#"{"choices":[{"message":{"content":"partial"}}]}"#.as_slice(),
        br#"{"choices":[{"message":{"content":"partial"},"finish_reason":null}]}"#.as_slice(),
        br#"{"choices":[{"message":{"content":"partial"},"finish_reason":""}]}"#.as_slice(),
        br#"{"choices":[{"message":{"tool_calls":[{"function":{"name":"f","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#.as_slice(),
        br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#.as_slice(),
        br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"f","arguments":{}}}]},"finish_reason":"tool_calls"}]}"#.as_slice(),
    ] {
        assert!(decode_response_events(W::OpenAiChat, input).is_err());
    }
    let call = br#"{"output":[{"type":"function_call","call_id":"c","name":"f"}]}"#;
    assert!(decode_response_events(W::OpenAiResponses, call).is_err());
    for (input, valid) in [
        (br#"{"model":"m","input":[{"type":"function_call","call_id":"c","name":"f","arguments":"{}"}]}"#.as_slice(), true),
        (br#"{"model":"m","input":[{"type":"function_call","call_id":"","name":"f","arguments":"{}"}]}"#.as_slice(), false),
        (br#"{"model":"m","input":[{"type":"function_call","call_id":"c","name":" ","arguments":"{}"}]}"#.as_slice(), false),
        (br#"{"model":"m","input":[{"type":"function_call_output","call_id":"","output":"x"}]}"#.as_slice(), false),
    ] {
        assert_eq!(decode_model_request(W::OpenAiResponses, input).is_ok(), valid);
    }
}
#[test]
fn gemini_rejects_malformed_responses() {
    for input in [
        &br#"{"modelVersion":"m","candidates":[]}"#[..],
        &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"args":{}}}]}}]}"#[..],
        &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":"x","args":[]}}]}}]}"#[..],
        &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":"x","id":7}}]}}]}"#[..],
    ] {
        assert!(decode_response_events(W::Gemini, input).is_err());
    }
}
#[test]
fn gemini_replays_function_call_thought_signature() {
    let input = br#"{"model":"m","contents":[{"role":"model","parts":[{"functionCall":{"id":"call-1","name":"one","args":{}},"thoughtSignature":"sig"},{"functionCall":{"id":"call-2","name":"two","args":{}}}]}]}"#;
    let request = decode_model_request(W::Gemini, input).unwrap();
    let encoded = String::from_utf8(encode_model_request(W::Gemini, &request).unwrap()).unwrap();
    assert!(encoded.contains(r#""thoughtSignature":"sig""#));
    assert_eq!(encoded.matches("thoughtSignature").count(), 1);
}
