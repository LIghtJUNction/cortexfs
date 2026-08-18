use std::{path::PathBuf, time::Duration};

use serde_json::json;

use super::decode;
use crate::config::Config;
use crate::error::{Error, Result};

fn config() -> Config {
    Config {
        bot_id: "bot".to_owned(),
        secret: "secret".to_owned(),
        allowed_users: vec!["user-1".to_owned()],
        allowed_groups: Vec::new(),
        socket: PathBuf::from("/run/cortexfs/channel/wecom-ws.sock"),
        reply_timeout: Duration::from_secs(1),
    }
}

#[test]
fn decodes_text_callback_and_preserves_identity() -> Result<()> {
    let frame = json!({
        "cmd": "aibot_msg_callback",
        "headers": {"req_id": "request-1"},
        "body": {
            "msgid": "message-1",
            "msgtype": "text",
            "chattype": "single",
            "from": {"userid": "user-1"},
            "text": {"content": "hello"}
        }
    });
    let event = decode(&frame, &config())?
        .ok_or_else(|| Error::Protocol("expected callback".to_owned()))?;
    assert_eq!(event.message.sender.id, "user-1");
    assert_eq!(event.message.body.text, "hello");
    assert_eq!(event.message.id, "message-1");
    assert_eq!(
        event
            .message
            .metadata
            .get("wecom_req_id")
            .map(String::as_str),
        Some("request-1")
    );
    Ok(())
}

#[test]
fn silently_drops_sender_outside_allowlist() -> Result<()> {
    let frame = json!({
        "cmd": "aibot_msg_callback",
        "headers": {"req_id": "request-1"},
        "body": {"msgtype":"text", "from":{"userid":"other"}, "text":{"content":"hi"}}
    });
    assert!(decode(&frame, &config())?.is_none());
    Ok(())
}
