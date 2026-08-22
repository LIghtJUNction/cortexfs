use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelDriverSession, ChannelFrameBody,
    ChannelRuntimeEvent, DeliveryReceipt,
};
use rumqttc::AsyncClient;

use crate::{
    config::Config,
    error::{Error, Result},
    message,
};

pub(super) async fn handle(
    config: &Config,
    client: &AsyncClient,
    session: &ChannelDriverSession,
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
            session.send_receipt(
                request_id,
                DeliveryReceipt::new(message.target, format!("mqtt-{topic}")),
            )?;
        }
        ChannelFrameBody::Command {
            request_id,
            session: session_id,
            command_id,
            command,
            target,
        } => {
            let result = match command {
                ChannelCommand::Invoke { name, payload } => {
                    crate::invoke::run(config, client, target.as_ref(), &name, &payload).await
                }
                _ => Err(Error::Config("MQTT command is unsupported".to_owned())),
            };
            session.send_command_result(
                request_id,
                session_id,
                command_id,
                ChannelCommandResult::from_value_result(result),
            )?;
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
