use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelCodec, MessageBody, MessageTarget, OutboundMessage, platform::qq::QqCodec,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{QqConfig, QqError, api};

pub(super) fn send(
    client: &Client,
    config: &QqConfig,
    message: &OutboundMessage,
) -> Result<(), QqError> {
    api::send(client, config, QqCodec.encode(message)?)
}

pub(super) fn run(
    client: &Client,
    config: &QqConfig,
    target: &MessageTarget,
    name: &str,
    payload: &Value,
) -> Result<Value, QqError> {
    if !matches!(
        name,
        "qq.send_markdown" | "qq.send_media" | "qq.send_keyboard" | "qq.send_group" | "qq.send_c2c"
    ) {
        return Err(QqError::Protocol("unsupported operation".to_owned()));
    }
    let kind = match name {
        "qq.send_group" => "group",
        "qq.send_c2c" => "c2c",
        _ => "guild",
    };
    let text = payload
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut metadata = BTreeMap::new();
    metadata.insert("qq.target_kind".to_owned(), kind.to_owned());
    let message = OutboundMessage {
        target: target.clone(),
        body: MessageBody::text(text.to_owned())?,
        metadata,
    };
    let mut request = QqCodec.encode(&message)?;
    if name == "qq.send_markdown" {
        let markdown = payload
            .get("markdown")
            .cloned()
            .unwrap_or_else(|| json!({"content":text}));
        request.body = json!({"content":text,"msg_type":2,"markdown":markdown}).to_string();
    } else if name == "qq.send_media" {
        request.body = json!({"content":text,"msg_type":7,"media":payload.get("media").ok_or(QqError::Protocol("media is missing".to_owned()))?}).to_string();
    } else if name == "qq.send_keyboard" {
        request.body = json!({"content":text,"msg_type":2,"keyboard":payload.get("keyboard").ok_or(QqError::Protocol("keyboard is missing".to_owned()))?}).to_string();
    }
    api::send(client, config, request)?;
    Ok(json!({"accepted":true,"channel":kind}))
}
