use std::{path::PathBuf, time::Duration};

use serde_json::json;

use crate::error::{Error, Result};

use super::*;

fn config(users: &[&str]) -> Config {
    Config {
        token: "token".to_owned(),
        api_base: "https://example.test".to_owned(),
        allowed_users: users.iter().map(|value| (*value).to_owned()).collect(),
        socket: PathBuf::from("/run/cortexfs/channel/wechat.sock"),
        poll_timeout: Duration::from_secs(40),
        reply_timeout: Duration::from_mins(1),
        channel_version: "test".to_owned(),
        wechat_uin: "test".to_owned(),
    }
}

#[test]
fn decodes_text_and_context_token() -> Result<()> {
    let value = json!({
        "from_user_id": "wx-user",
        "message_id": 42,
        "create_time_ms": 1000,
        "context_token": "ctx",
        "item_list": [{"type": 1, "text_item": {"text": " hi "}}]
    });
    let incoming = decode(&value, &config(&["wx-user"]))?
        .ok_or_else(|| Error::Protocol("expected a text message".to_owned()))?;
    assert_eq!(incoming.message.body.text, "hi");
    assert_eq!(
        incoming.message.target.conversation.as_str(),
        "user:wx-user"
    );
    assert_eq!(incoming.context_token, "ctx");
    Ok(())
}

#[test]
fn decodes_voice_transcript_and_denies_unknown_user() -> Result<()> {
    let value = json!({
        "from_user_id": "wx-user",
        "item_list": [{"type": 3, "voice_item": {"text": "voice"}}]
    });
    let incoming = decode(&value, &config(&["wx-user"]))?
        .ok_or_else(|| Error::Protocol("expected a voice message".to_owned()))?;
    assert_eq!(incoming.message.body.text, "voice");
    assert!(decode(&value, &config(&["other"]))?.is_none());
    Ok(())
}

#[test]
fn ignores_media_without_text_projection() -> Result<()> {
    let value = json!({
        "from_user_id": "wx-user",
        "item_list": [{"type": 2, "image_item": {"media": "..."}}]
    });
    assert!(decode(&value, &config(&["wx-user"]))?.is_none());
    Ok(())
}
