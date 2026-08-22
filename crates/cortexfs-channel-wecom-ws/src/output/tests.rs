use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use super::{reconnect, reply_frames, send};

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

#[tokio::test]
async fn sends_text_and_reports_a_closed_queue() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    assert!(send(&sender, "hello".to_owned()).await.is_ok());
    assert_eq!(receiver.recv().await, Some(Message::Text("hello".into())));
    drop(receiver);
    let error = send(&sender, "closed".to_owned()).await;
    assert!(matches!(
        error,
        Err(crate::error::Error::Protocol(ref message))
            if message == "WeCom output queue closed"
    ));
}

#[test]
fn recognizes_server_disconnect_event() {
    let frame = serde_json::json!({"body":{"event":{"eventtype":"disconnected_event"}}});
    assert!(reconnect(&frame));
}
