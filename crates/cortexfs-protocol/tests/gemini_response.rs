use cortexfs_protocol::{WireProtocol, decode_response_events};
#[test]
fn gemini_rejects_malformed_responses() {
    for input in [
        &br#"{"modelVersion":"m","candidates":[]}"#[..],
        &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"args":{}}}]}}]}"#[..],
        &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":7}}]}}]}"#[..],
        &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":""}}]}}]}"#[..],
        &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":"x","args":[]}}]}}]}"#[..],
        &br#"{"modelVersion":"m","candidates":[{"content":{"parts":[{"functionCall":{"name":"x","id":7}}]}}]}"#[..],
    ] {
        assert!(decode_response_events(WireProtocol::Gemini, input).is_err());
    }
}
