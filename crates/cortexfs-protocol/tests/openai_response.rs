#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol, decode_response_events};
    #[test]
    fn openai_rejects_incomplete_responses() {
        assert!(decode_response_events(WireProtocol::OpenAiChat, br#"{"choices":[]}"#).is_err());
        assert!(decode_response_events(WireProtocol::OpenAiChat, br#"{"choices":[{}]}"#).is_err());
        for call in [br#"{"output":[{"type":"function_call","name":"f","arguments":"{}"}]}"#.as_slice(), br#"{"output":[{"type":"function_call","call_id":"c","arguments":"{}"}]}"#.as_slice(), br#"{"output":[{"type":"function_call","call_id":"c","name":"f"}]}"#.as_slice()] {
            assert!(decode_response_events(WireProtocol::OpenAiResponses, call).is_err());
        }
    }
}
