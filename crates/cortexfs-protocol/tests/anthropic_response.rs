#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol, decode_response_events};
    #[test]
    fn malformed_anthropic_tool_use_is_rejected() {
        for input in [
            br#"{"content":[{"type":"tool_use","name":"x","input":{}}]}"#.as_slice(),
            br#"{"content":[{"type":"tool_use","id":"1","input":{}}]}"#.as_slice(),
            br#"{"content":[{"type":"tool_use","id":"1","name":"x"}]}"#.as_slice(),
        ] {
            assert!(decode_response_events(WireProtocol::Anthropic, input).is_err());
        }
    }
}
