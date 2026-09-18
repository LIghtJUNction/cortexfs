#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol, decode_response_events};
    #[test]
    fn openai_rejects_incomplete_responses() {
        let chat = WireProtocol::OpenAiChat;
        let malformed = [
            br#"{"choices":[]}"#.as_slice(),
            br#"{"choices":[{}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"function":{"name":"f","arguments":"{}"}}]}}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"id":"","function":{"name":"f","arguments":"{}"}}]}}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"arguments":"{}"}}]}}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"","arguments":"{}"}}]}}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"f"}}]}}]}"#.as_slice(),
            br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"f","arguments":{}}}]}}]}"#.as_slice(),
        ];
        for input in malformed {
            assert!(decode_response_events(chat, input).is_err());
        }
        let call = br#"{"output":[{"type":"function_call","call_id":"c","name":"f"}]}"#;
        assert!(decode_response_events(WireProtocol::OpenAiResponses, call).is_err());
    }
}
