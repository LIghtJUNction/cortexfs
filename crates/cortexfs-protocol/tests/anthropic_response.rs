mod tests {
    use cortexfs_protocol::{WireProtocol, decode_response_events};
    #[test]
    fn malformed_anthropic_tool_use_is_rejected() {
        let input = br#"{"content":[{"type":"tool_use","id":"1","name":"x","input":[]}]}"#;
        assert!(decode_response_events(WireProtocol::Anthropic, input).is_err());
    }
}
