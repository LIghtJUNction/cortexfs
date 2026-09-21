#[cfg(test)]
mod tests {
    use cortexfs_protocol::{WireProtocol, decode_model_request, encode_model_request};

    #[test]
    fn openai_image_request_roundtrips() -> Result<(), cortexfs_protocol::ConversionError> {
        let input = br#"{"model":"m","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.invalid/i.png"}}]}]}"#;
        let protocol = WireProtocol::OpenAiChat;
        let request = decode_model_request(protocol, input)?;
        let encoded = encode_model_request(protocol, &request)?;
        assert_eq!(decode_model_request(protocol, &encoded)?, request);
        Ok(())
    }
}
