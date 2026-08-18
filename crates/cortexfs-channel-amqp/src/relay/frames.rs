use std::collections::BTreeMap;

use cortexfs_channels::{ChannelFrameBody, ChannelRuntimeEvent, DeliveryReceipt};
use lapin::{
    message::Delivery,
    options::{BasicAckOptions, BasicNackOptions},
};

use crate::{
    config::Config,
    error::{Error, Result},
    message, socket,
};

pub(super) async fn handle(
    config: &Config,
    channel: &lapin::Channel,
    session: &socket::Session,
    pending: &mut BTreeMap<String, Delivery>,
    frame: ChannelFrameBody,
) -> Result<()> {
    match frame {
        ChannelFrameBody::Deliver {
            request_id,
            message,
        } => {
            if let Some(delivery) = pending.remove(&request_id) {
                if let Err(error) = message::publish(
                    channel,
                    &message,
                    delivery.exchange.as_str(),
                    delivery.routing_key.as_str(),
                )
                .await
                {
                    reject(config, &delivery).await?;
                    return Err(error);
                }
                delivery.ack(BasicAckOptions::default()).await?;
            }
        }
        ChannelFrameBody::Outbound {
            request_id,
            message,
        } => {
            let target = message.target.clone();
            message::publish(
                channel,
                &message,
                config.exchange.as_str(),
                target.conversation.as_str(),
            )
            .await?;
            session.receipt(
                request_id,
                DeliveryReceipt {
                    channel: target.channel.clone(),
                    message_id: format!("amqp-{}", target.conversation),
                    target,
                    timestamp_ms: None,
                },
            )?;
        }
        ChannelFrameBody::Event {
            event: ChannelRuntimeEvent::Disconnected,
        } => return Err(Error::Closed),
        ChannelFrameBody::Error {
            request_id: Some(request_id),
            ..
        } => {
            if let Some(delivery) = pending.remove(&request_id) {
                reject(config, &delivery).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) async fn reject(config: &Config, delivery: &Delivery) -> Result<()> {
    delivery
        .nack(BasicNackOptions {
            requeue: config.durable_ack && !delivery.redelivered,
            ..Default::default()
        })
        .await?;
    Ok(())
}
