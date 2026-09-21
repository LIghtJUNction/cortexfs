#[cfg(test)]
mod tests {
    use cortexfs_protocol::{
        WireProtocol, decode_model_request, decode_response_events, encode_model_request,
    };
    use serde_json::Value;

    #[test]
    fn gemini_rejects_malformed_responses() {
        for input in [
            &br#"{"modelVersion":"m","candidates":[]}"#[..],
            &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"args":{}}}]}}]}"#[..],
            &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":"x","args":[]}}]}}]}"#[..],
            &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":"x","id":7}}]}}]}"#[..],
        ] {
            assert!(decode_response_events(WireProtocol::Gemini, input).is_err());
        }
    }

    #[test]
    fn gemini_replays_function_call_thought_signature() -> Result<(), Box<dyn std::error::Error>> {
        let input = br#"{"model":"m","contents":[{"role":"model","parts":[{"functionCall":{"id":"call-1","name":"one","args":{}},"thoughtSignature":"sig"},{"functionCall":{"id":"call-2","name":"two","args":{}}}]}]}"#;
        let request = decode_model_request(WireProtocol::Gemini, input)?;
        let encoded = encode_model_request(WireProtocol::Gemini, &request)?;
        let value: Value = serde_json::from_slice(&encoded)?;
        assert_eq!(
            value
                .pointer("/contents/0/parts/0/thoughtSignature")
                .and_then(Value::as_str),
            Some("sig")
        );
        assert!(value.pointer("/contents/0/parts/1/thoughtSignature").is_none());
        Ok(())
    }
}
