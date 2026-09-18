#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol, decode_response_events};
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
    }
}
