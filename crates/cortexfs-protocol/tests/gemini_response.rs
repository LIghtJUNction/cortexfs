use cortexfs_protocol::{WireProtocol, decode_response_events};

#[test]
fn gemini_rejects_empty_candidates() {
    let input = br#"{"modelVersion":"gemini-model","candidates":[]}"#;
    assert!(decode_response_events(WireProtocol::Gemini, input).is_err());
}
