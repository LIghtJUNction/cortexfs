#![expect(
    clippy::redundant_pub_crate,
    reason = "message conversion is private driver plumbing"
)]

use std::collections::BTreeMap;

use lapin::{BasicProperties, options::BasicPublishOptions};

use crate::error::{Error, Result};

pub(crate) async fn publish(
    channel: &lapin::Channel,
    message: &cortexfs_channels::OutboundMessage,
    fallback_exchange: &str,
    fallback_key: &str,
) -> Result<()> {
    if !message.body.attachments.is_empty() || message.body.text.is_empty() {
        return Err(Error::Message(
            "AMQP outbound attachments are not supported".to_owned(),
        ));
    }
    let exchange = message
        .metadata
        .get("amqp_exchange")
        .map_or(fallback_exchange, String::as_str);
    let routing_key = message
        .metadata
        .get("amqp_routing_key")
        .map_or(fallback_key, String::as_str);
    channel
        .basic_publish(
            exchange,
            routing_key,
            BasicPublishOptions::default(),
            message.body.text.as_bytes(),
            BasicProperties::default(),
        )
        .await?
        .await?;
    Ok(())
}

pub(crate) fn decode(
    delivery: &lapin::message::Delivery,
) -> Result<cortexfs_channels::InboundMessage> {
    let sender = delivery
        .properties
        .app_id()
        .as_ref()
        .map_or_else(|| "amqp".to_owned(), ToString::to_string);
    let conversation = delivery
        .properties
        .correlation_id()
        .as_ref()
        .map_or_else(|| sender.clone(), ToString::to_string);
    let id = delivery.properties.message_id().as_ref().map_or_else(
        || format!("amqp-{}-{}", delivery.routing_key, delivery.delivery_tag),
        ToString::to_string,
    );
    let text = String::from_utf8(delivery.data.clone())
        .map_err(|_error| Error::Message("AMQP delivery is not UTF-8 text".to_owned()))?;
    let target = cortexfs_channels::MessageTarget {
        channel: cortexfs_channels::ChannelId::from_static("amqp"),
        conversation: cortexfs_channels::ConversationId::new(conversation)?,
        thread: delivery
            .properties
            .correlation_id()
            .as_ref()
            .map(ToString::to_string),
        reply_to: None,
    };
    Ok(cortexfs_channels::InboundMessage {
        id,
        target,
        sender: cortexfs_channels::Participant {
            id: sender,
            ..Default::default()
        },
        body: cortexfs_channels::MessageBody::text(text)?,
        timestamp_ms: None,
        metadata: metadata(delivery),
    })
}

fn metadata(delivery: &lapin::message::Delivery) -> BTreeMap<String, String> {
    [
        ("amqp_exchange".to_owned(), delivery.exchange.to_string()),
        (
            "amqp_routing_key".to_owned(),
            delivery.routing_key.to_string(),
        ),
    ]
    .into_iter()
    .collect()
}
