use std::collections::BTreeMap;

use cortexfs_channels::MessageTarget;
use lapin::{
    Channel,
    message::Delivery,
    options::{BasicAckOptions, BasicNackOptions, BasicPublishOptions, QueueBindOptions},
    types::FieldTable,
};
use serde_json::{Value, json};

use crate::{config::Config, error::Result};

#[expect(clippy::redundant_pub_crate, reason = "private driver helper")]
pub(crate) async fn run(
    config: &Config,
    channel: &Channel,
    pending: &mut BTreeMap<String, Delivery>,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value> {
    match name {
        "amqp.publish" => {
            let exchange = text(payload, "exchange").unwrap_or(&config.exchange);
            let key = text(payload, "routing_key")
                .or_else(|| target.map(|item| item.conversation.as_str()))
                .unwrap_or("");
            let body = text(payload, "body").ok_or_else(|| error("body is missing"))?;
            channel
                .basic_publish(
                    exchange,
                    key,
                    BasicPublishOptions::default(),
                    body.as_bytes(),
                    lapin::BasicProperties::default(),
                )
                .await?
                .await?;
            Ok(json!({"accepted":true}))
        }
        "amqp.subscribe" => {
            let key =
                text(payload, "routing_key").ok_or_else(|| error("routing_key is missing"))?;
            channel
                .queue_bind(
                    &config.queue,
                    &config.exchange,
                    key,
                    QueueBindOptions::default(),
                    FieldTable::default(),
                )
                .await?;
            Ok(json!({"accepted":true,"routing_key":key}))
        }
        "amqp.ack" | "amqp.reject" => {
            let id = text(payload, "message_id").ok_or_else(|| error("message_id is missing"))?;
            let delivery = pending
                .remove(id)
                .ok_or_else(|| error("message is not pending"))?;
            if name == "amqp.ack" {
                delivery.ack(BasicAckOptions::default()).await?;
            } else {
                delivery.nack(BasicNackOptions::default()).await?;
            }
            Ok(json!({"accepted":true}))
        }
        _ => Err(error("unsupported operation")),
    }
}

fn text<'a>(payload: &'a Value, name: &str) -> Option<&'a str> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn error(message: &str) -> crate::error::Error {
    crate::error::Error::Config(message.to_owned())
}
