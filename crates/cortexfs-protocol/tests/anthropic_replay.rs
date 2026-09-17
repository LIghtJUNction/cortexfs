use cortexfs_protocol::{WireProtocol, decode_model_request, encode_model_request};
#[test]
fn tool_use_replay_stays_out_of_content() -> Result<(), Box<dyn std::error::Error>> {
    let input = br#"{"model":"claude-model","max_tokens":32,"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"secret","signature":"sig"},{"type":"text","text":"visible"},{"type":"tool_use","id":"call-1","name":"lookup","input":{"q":"rust"}}]}]}"#;
    let request = decode_model_request(WireProtocol::Anthropic, input)?;
    let encoded = String::from_utf8(encode_model_request(WireProtocol::Anthropic, &request)?)?;
    assert_eq!(encoded.matches("\"type\":\"tool_use\"").count(), 1);
    assert!(!encoded.contains("anthropic.tool_use"));
    Ok(())
}
