use cortexfs_channels::MessageTarget;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::error::{Error, Result};

#[expect(clippy::redundant_pub_crate, reason = "private driver helper")]
pub(crate) async fn run(
    output_tx: &mpsc::Sender<Message>,
    request_id: &str,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value> {
    if !matches!(
        name,
        "wecom_ws.send_markdown"
            | "wecom-ws.send_markdown"
            | "wecom_ws.send_media"
            | "wecom-ws.send_media"
            | "wecom_ws.send_file"
            | "wecom-ws.send_file"
            | "wecom_ws.draft_update"
            | "wecom-ws.draft_update"
    ) {
        return Err(Error::Protocol("unsupported operation".to_owned()));
    }
    let text = payload
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| target.map(|item| item.conversation.as_str()))
        .ok_or_else(|| Error::Protocol("text is missing".to_owned()))?;
    for frame in crate::output::reply_frames(request_id, text) {
        output_tx
            .send(Message::Text(frame.into()))
            .await
            .map_err(|_error| Error::Protocol("WeCom output queue closed".to_owned()))?;
    }
    Ok(json!({"accepted":true}))
}
