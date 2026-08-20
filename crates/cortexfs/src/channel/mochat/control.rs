use super::{MochatConfig, api};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler, CodecTransport},
};
use cortexfs_channels::{
    Attachment, ChannelCapabilities, ChannelId, MessageBody, MessageTarget, OutboundMessage,
    platform::mochat::MochatCodec,
};
use reqwest::blocking::Client;
pub(super) fn start(
    config: &MochatConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<ChannelControl, super::MochatError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("mochat"),
        bridge.clone(),
        ChannelCapabilities {
            send: true,
            tool_control: true,
            polling: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(CodecHandler::new(Box::new(Transport {
            client: client.clone(),
            config: config.clone(),
        }))),
    ))
    .map_err(|error| super::MochatError::Protocol(error.to_string()))
}

struct Transport {
    client: Client,
    config: MochatConfig,
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn cortexfs_channels::ChannelCodec {
        &MochatCodec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), cortexfs::channel::control::ChannelControlError> {
        api::send(&self.client, &self.config, request).map_err(|error| {
            cortexfs::channel::control::ChannelControlError::Operation(error.to_string())
        })
    }

    fn invoke(
        &mut self,
        target: Option<&MessageTarget>,
        name: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, cortexfs::channel::control::ChannelControlError> {
        if name == "mochat.cursor" {
            let cursor = payload
                .get("cursor")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| fail("cursor is missing"))?;
            return Ok(serde_json::json!({"cursor":cursor}));
        }
        let target = target.ok_or_else(|| fail("target is missing"))?;
        if name == "mochat.update" {
            let message_id = payload
                .get("message_id")
                .and_then(serde_json::Value::as_str)
                .or(target.reply_to.as_deref())
                .ok_or_else(|| fail("message_id is missing"))?;
            let text = payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| fail("text is missing"))?;
            self.send(cortexfs_channels::OutboundRequest {
                method: "POST".to_owned(),
                path: "api/message/update".to_owned(),
                content_type: "application/json".to_owned(),
                body: serde_json::json!({"messageId":message_id,"content":{"text":text}})
                    .to_string(),
                headers: std::collections::BTreeMap::new(),
            })?;
            return Ok(serde_json::json!({"accepted":true}));
        }
        if name != "mochat.send_media" {
            return Err(fail("unsupported operation"));
        }
        let attachments: Vec<Attachment> = serde_json::from_value(
            payload
                .get("attachments")
                .cloned()
                .unwrap_or(serde_json::Value::Array(Vec::new())),
        )
        .map_err(|error| fail(&error.to_string()))?;
        let text = payload
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let message = OutboundMessage {
            target: target.clone(),
            body: MessageBody::with_attachments(text, attachments)
                .map_err(|error| fail(&error.to_string()))?,
            metadata: std::collections::BTreeMap::new(),
        };
        let request = self
            .codec()
            .encode(&message)
            .map_err(|error| fail(&error.to_string()))?;
        self.send(request)?;
        Ok(serde_json::json!({"accepted":true}))
    }
}

fn fail(message: &str) -> cortexfs::channel::control::ChannelControlError {
    cortexfs::channel::control::ChannelControlError::Operation(message.to_owned())
}
