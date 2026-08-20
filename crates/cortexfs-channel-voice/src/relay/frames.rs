use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelFrameBody, ChannelRuntimeEvent, DeliveryReceipt,
};
use reqwest::Client;

use crate::{
    config::Config,
    error::{Error, Result},
    provider::{self, Calls},
    socket::Session,
};

pub(super) async fn handle(
    config: &Config,
    client: &Client,
    session: &Session,
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
            session.send_frame(ChannelFrameBody::CommandResult {
                request_id,
                session: session_id,
                command_id,
                result: result.map_or_else(
                    |error| ChannelCommandResult::Rejected {
                        reason: error.to_string(),
                    },
                    |payload| ChannelCommandResult::Value { payload },
                ),
            })?;
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
    session.receipt(
        request_id,
        DeliveryReceipt {
            channel: target.channel.clone(),
            message_id: format!("voice-{id}"),
            target,
            timestamp_ms: None,
        },
    )
}
