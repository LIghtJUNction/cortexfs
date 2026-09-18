#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol, decode_response_events};
    #[test]
    fn openai_rejects_incomplete_responses() {
        assert!(decode_response_events(WireProtocol::OpenAiChat, br#"{"choices":[]}"#).is_err());
        let missing = br#"{"choices":[{"finish_reason":"stop"}]}"#;
        assert!(decode_response_events(WireProtocol::OpenAiChat, missing).is_err());
        let call = br#"{"output":[{"type":"function_call","call_id":"c","name":"f"}]}"#;
        assert!(decode_response_events(WireProtocol::OpenAiResponses, call).is_err());
    }
}
