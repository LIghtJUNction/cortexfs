use serde_json::Value;

use super::{reconnect, reply_frames};

#[test]
fn chunks_reply_without_breaking_utf8() -> Result<(), serde_json::Error> {
    let frames = reply_frames("request", &"界".repeat(3_000));
    assert!(frames.len() > 1);
    for frame in frames {
        let value: Value = serde_json::from_str(&frame)?;
        assert_eq!(
            value.pointer("/headers/req_id").and_then(Value::as_str),
            Some("request")
        );
    }
    Ok(())
}

#[test]
fn recognizes_server_disconnect_event() {
    let frame = serde_json::json!({"body":{"event":{"eventtype":"disconnected_event"}}});
    assert!(reconnect(&frame));
}
