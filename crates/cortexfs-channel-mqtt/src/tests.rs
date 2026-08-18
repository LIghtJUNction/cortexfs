use super::message;

#[test]
fn decodes_json_envelope_without_provider_types() {
    let result = message::decode(
        "agents/one",
        br#"{"id":"m-1","sender":"user-1","conversation":"conv-1","text":"hello","timestamp_ms":42}"#,
    );
    assert!(result.is_ok());
    let Some(message) = result.ok() else { return };

    assert_eq!(message.id, "m-1");
    assert_eq!(message.sender.id, "user-1");
    assert_eq!(message.target.conversation.as_str(), "conv-1");
    assert_eq!(message.body.text, "hello");
    assert_eq!(message.timestamp_ms, Some(42));
    assert_eq!(
        message.metadata.get("mqtt.topic"),
        Some(&"agents/one".to_owned())
    );
}

#[test]
fn decodes_plain_payload_with_topic_identity() {
    let result = message::decode("events/room", b"hello");
    assert!(result.is_ok());
    let Some(message) = result.ok() else { return };

    assert_eq!(message.target.conversation.as_str(), "events/room");
    assert_eq!(message.sender.id, "mqtt");
    assert_eq!(message.body.text, "hello");
    assert!(message.id.starts_with("mqtt-"));
}

#[test]
fn rejects_non_utf8_payload() {
    let result = message::decode("events/room", &[0xff]);
    assert!(result.is_err());
    let Some(error) = result.err() else { return };

    assert!(error.to_string().contains("not UTF-8"));
}
