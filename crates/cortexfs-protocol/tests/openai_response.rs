#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol, decode_response_events};

    #[test]
    fn openai_chat_rejects_incomplete_choices() {
        let empty = br#"{"id":"run","model":"model","choices":[]}"#;
        assert!(decode_response_events(WireProtocol::OpenAiChat, empty).is_err());
        let missing = br#"{"id":"run","model":"model","choices":[{"finish_reason":"stop"}]}"#;
        assert!(decode_response_events(WireProtocol::OpenAiChat, missing).is_err());
    }
}
