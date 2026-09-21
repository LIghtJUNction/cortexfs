use cortexfs_protocol::{
    Content, ContentPart, Message, ModelRequest, WireProtocol, decode_model_request,
    encode_model_request,
};
use serde_json::{Value, json};

#[test]
fn image_part_uses_openai_object_shape_and_roundtrips() -> Result<(), Box<dyn std::error::Error>> {
    let expected = Content::Parts(vec![ContentPart::Image {
        uri: "https://example.invalid/image.png".to_owned(),
        mime: None,
    }]);
    let mut message = Message::user("");
    message.content = expected.clone();
    let request = ModelRequest::new("chat-model", vec![message]);
    let encoded = encode_model_request(WireProtocol::OpenAiChat, &request)?;
    let value: Value = serde_json::from_slice(&encoded)?;
    assert_eq!(
        value.pointer("/messages/0/content/0/image_url/url"),
        Some(&json!("https://example.invalid/image.png"))
    );
    let decoded = decode_model_request(WireProtocol::OpenAiChat, &encoded)?;
    assert_eq!(decoded.messages[0].content, expected);
    Ok(())
}
