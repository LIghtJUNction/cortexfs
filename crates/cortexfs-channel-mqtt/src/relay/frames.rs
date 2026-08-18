use cortexfs_channels::{
    ChannelCommandResult, ChannelFrameBody, ChannelRuntimeEvent, DeliveryReceipt,
};
use rumqttc::AsyncClient;

use crate::{
    config::Config,
    error::{Error, Result},
    message,
    socket::Session,
};

pub(super) async fn handle(
    config: &Config,
    client: &AsyncClient,
    session: &Session,
    frame: ChannelFrameBody,
) -> Result<()> {
    match frame {
        ChannelFrameBody::Deliver {
            request_id,
            message,
        }
        | ChannelFrameBody::Outbound {
            request_id,
            message,
        } => {
            let topic = message::publish(client, config, &message).await?;
            session.receipt(
                request_id,
                DeliveryReceipt {
                    channel: message.target.channel.clone(),
                    message_id: format!("mqtt-{topic}"),
                    target: message.target,
                    timestamp_ms: None,
                },
            )?;
        }
        ChannelFrameBody::Command {
            request_id,
            session: session_id,
            command_id,
            ..
        } => {
            session.send_frame(ChannelFrameBody::CommandResult {
                request_id,
                session: session_id,
                command_id,
                result: ChannelCommandResult::Rejected {
                    reason: "MQTT has no interactive command reply path".to_owned(),
                },
            })?;
        }
        ChannelFrameBody::Event {
            event: ChannelRuntimeEvent::Disconnected,
        } => {
            return Err(Error::Closed);
        }
        ChannelFrameBody::Error {
            request_id: Some(request_id),
            message,
            ..
        } => {
            return Err(Error::Config(format!(
                "runtime rejected {request_id}: {message}"
            )));
        }
        _ => {}
    }
    Ok(())
}
