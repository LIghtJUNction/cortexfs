use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelDriverSession, ChannelFrameBody,
    ChannelRuntimeEvent, DeliveryReceipt,
};
use reqwest::Client;

use crate::{
    config::Config,
    error::{Error, Result},
    provider::{self, Calls},
};

pub(super) async fn handle(
    config: &Config,
    client: &Client,
    session: &ChannelDriverSession,
    calls: &mut Calls,
    frame: ChannelFrameBody,
) -> Result<()> {
    let (request_id, message) = match frame {
        ChannelFrameBody::Deliver {
            request_id,
            message,
        }
        | ChannelFrameBody::Outbound {
            request_id,
            message,
        } => (request_id, message),
        ChannelFrameBody::Event {
            event: ChannelRuntimeEvent::Disconnected,
        } => {
            return Err(Error::Protocol("channel driver disconnected".to_owned()));
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
                    crate::invoke::run(config, client, calls, target.as_ref(), &name, &payload)
                        .await
                }
                _ => Err(Error::Protocol("voice command is unsupported".to_owned())),
            };
            session.send_command_result(
                request_id,
                session_id,
                command_id,
                ChannelCommandResult::from_value_result(result),
            )?;
            return Ok(());
        }
        ChannelFrameBody::Error {
            request_id: Some(request_id),
            ..
        } => {
            calls.retain(|_, call| call.id != request_id);
            return Ok(());
        }
        _ => return Ok(()),
    };
    let target = message.target.clone();
    let id = provider::send(config, client, calls, &message).await?;
    Ok(session.send_receipt(
        request_id,
        DeliveryReceipt::new(target, format!("voice-{id}")),
    )?)
}
