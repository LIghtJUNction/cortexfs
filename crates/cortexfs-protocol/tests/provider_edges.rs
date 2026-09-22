#[cfg(test)]
mod tests {
    use cortexfs_protocol::{self as p, WireProtocol as W};
    #[test]
    fn provider_edges_conform() -> Result<(), p::ConversionError> {
        for (protocol, input) in [
            (W::Anthropic, &br#"{"content":[{"type":"tool_use","id":0}]}"#[..]),
            (W::OpenAiResponses, &br#"{"output":[{"type":"function_call","call_id":"c","name":"f"}]}"#[..]),
            (W::Gemini, &br#"{"modelVersion":"m","candidates":[]}"#[..]),
            (W::Gemini, &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"args":{}}}]}}]}"#[..]),
            (W::Gemini, &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":"x","args":[]}}]}}]}"#[..]),
            (W::Gemini, &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":"x","id":7}}]}}]}"#[..]),
            (W::OpenAiChat, &br#"{"choices":[]}"#[..]),
            (W::OpenAiChat, &br#"{"choices":[{}]}"#[..]),
            (W::OpenAiChat, &br#"{"choices":[{"message":{"content":"partial"}}]}"#[..]),
            (W::OpenAiChat, &br#"{"choices":[{"message":{"content":"partial"},"finish_reason":null}]}"#[..]),
            (W::OpenAiChat, &br#"{"choices":[{"message":{"content":"partial"},"finish_reason":""}]}"#[..]),
            (W::OpenAiChat, &br#"{"choices":[{"message":{"tool_calls":[{"function":{"name":"f","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#[..]),
            (W::OpenAiChat, &br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#[..]),
            (W::OpenAiChat, &br#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"f","arguments":{}}}]},"finish_reason":"tool_calls"}]}"#[..]),
        ] {
            assert!(p::decode_response_events(protocol, input).is_err());
        }
        for (input, valid) in [
            (br#"{"model":"m","input":[{"type":"function_call","call_id":"c","name":"f","arguments":"{}"}]}"#.as_slice(), true),
            (br#"{"model":"m","input":[{"type":"function_call","call_id":"","name":"f","arguments":"{}"}]}"#.as_slice(), false),
            (br#"{"model":"m","input":[{"type":"function_call","call_id":"c","name":" ","arguments":"{}"}]}"#.as_slice(), false),
            (br#"{"model":"m","input":[{"type":"function_call_output","call_id":"","output":"x"}]}"#.as_slice(), false),
        ] {
            assert_eq!(p::decode_model_request(W::OpenAiResponses, input).is_ok(), valid);
        }
        let input = br#"{"model":"m","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.invalid/i.png"}}]}]}"#;
        let request = p::decode_model_request(W::OpenAiChat, input)?;
        let encoded = p::encode_model_request(W::OpenAiChat, &request)?;
        assert_eq!(p::decode_model_request(W::OpenAiChat, &encoded)?, request);
        let input = br#"{"model":"m","contents":[{"role":"model","parts":[{"functionCall":{"name":"same","args":{"n":1}},"thoughtSignature":"sig"},{"functionCall":{"name":"same","args":{"n":2}}}]}]}"#;
        let request = p::decode_model_request(W::Gemini, input)?;
        let encoded = p::encode_model_request(W::Gemini, &request)?;
        let encoded = String::from_utf8_lossy(&encoded);
        assert!(!encoded.contains("\"model\":"));
        assert!(encoded.contains("\"thoughtSignature\":\"sig\""));
        assert_eq!(encoded.matches("thoughtSignature").count(), 1);
        Ok(())
    }
}
