use cortexfs_channels::{
    ChannelCodec, MessageBody, OutboundMessage, platform::twitter::TwitterCodec,
};
use reqwest::blocking::Client;
use serde_json::Value;

use super::super::{TwitterConfig, api};
use crate::channel::control::{ChannelControlError, CodecTransport};

pub(super) struct Transport {
    client: Client,
    config: TwitterConfig,
    bot_id: String,
}

impl Transport {
    pub(super) fn new(client: &Client, config: &TwitterConfig, bot_id: &str) -> Self {
        Self {
            client: client.clone(),
            config: config.clone(),
            bot_id: bot_id.to_owned(),
        }
    }
}

impl CodecTransport for Transport {
    fn codec(&self) -> &dyn ChannelCodec {
        &TwitterCodec
    }

    fn send(
        &mut self,
        request: cortexfs_channels::OutboundRequest,
    ) -> Result<(), ChannelControlError> {
        api::send(&self.client, &self.config, request)
            .map(|_id| ())
            .map_err(|error| fail(&error.to_string()))
    }

    fn invoke(
        &mut self,
        target: Option<&cortexfs_channels::MessageTarget>,
        name: &str,
        payload: &Value,
    ) -> Result<Value, ChannelControlError> {
        match name {
            "twitter.post" | "twitter.reply" => self.post(target, name, payload),
            "twitter.like" => super::ops::like(&self.client, &self.config, &self.bot_id, payload),
            "twitter.search_mentions" => {
                super::ops::search(&self.client, &self.config, &self.bot_id)
            }
            "twitter.send_dm" => super::ops::dm(&self.client, &self.config, payload),
            _ => Err(fail("unsupported operation")),
        }
    }
}

impl Transport {
    fn post(
        &self,
        target: Option<&cortexfs_channels::MessageTarget>,
        name: &str,
        payload: &Value,
    ) -> Result<Value, ChannelControlError> {
        let mut target = target.cloned().ok_or_else(|| fail("target is missing"))?;
        if name == "twitter.reply" {
            target.reply_to = Some(string(payload, "tweet_id")?.to_owned());
        }
        let text = string(payload, "text")?;
        let message = OutboundMessage {
            target,
            body: MessageBody::text(text.to_owned()).map_err(|error| fail(&error.to_string()))?,
            metadata: std::collections::BTreeMap::new(),
        };
        let request = TwitterCodec
            .encode(&message)
            .map_err(|error| fail(&error.to_string()))?;
        let id = api::send(&self.client, &self.config, request)
            .map_err(|error| fail(&error.to_string()))?;
        Ok(serde_json::json!({"accepted":true,"id":id}))
    }
}

fn string<'a>(payload: &'a Value, name: &'static str) -> Result<&'a str, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| fail(&format!("{name} is missing")))
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
