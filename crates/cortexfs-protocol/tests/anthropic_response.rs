#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol as W, decode_response_events as decode};
    #[test]
    fn malformed_anthropic_tool_use_is_rejected() {
        assert!(decode(W::Anthropic, br#"{"content":[{"type":"tool_use","id":0}]}"#).is_err());
    }
}
