#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol, decode_model_request, decode_response_events};
    #[test]
    fn openai_rejects_incomplete_responses() {
        for input in [
            br#"{"choices":[]}"#.as_slice(),
            br#"{"choices":[{}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"function":{"name":"f","arguments":"{}"}}]}}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"","arguments":"{}"}}]}}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"f","arguments":{}}}]}}]}"#.as_slice(),
        ] {
            assert!(decode_response_events(WireProtocol::OpenAiChat, input).is_err());
        }
        let call = br#"{"output":[{"type":"function_call","call_id":"c","name":"f"}]}"#;
        assert!(decode_response_events(WireProtocol::OpenAiResponses, call).is_err());
        for (input, valid) in [
            (br#"{"model":"m","input":[{"type":"function_call","call_id":"c","name":"f","arguments":"{}"}]}"#.as_slice(), true),
            (br#"{"model":"m","input":[{"type":"function_call","call_id":"","name":"f","arguments":"{}"}]}"#.as_slice(), false),
            (br#"{"model":"m","input":[{"type":"function_call","call_id":"c","name":" ","arguments":"{}"}]}"#.as_slice(), false),
            (br#"{"model":"m","input":[{"type":"function_call_output","call_id":"","output":"x"}]}"#.as_slice(), false),
        ] {
            assert_eq!(decode_model_request(WireProtocol::OpenAiResponses, input).is_ok(), valid);
        }
    }
}
