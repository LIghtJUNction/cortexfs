use cortexfs_protocol::{decode_model_request, encode_model_request, WireProtocol};

#[test]
fn openai_image_request_roundtrips() {
    let input = br#"{"model":"m","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.invalid/i.png"}}]}]}"#;
    let request = decode_model_request(WireProtocol::OpenAiChat, input).unwrap();
    let encoded = encode_model_request(WireProtocol::OpenAiChat, &request).unwrap();
    assert_eq!(decode_model_request(WireProtocol::OpenAiChat, &encoded).unwrap(), request);
}
