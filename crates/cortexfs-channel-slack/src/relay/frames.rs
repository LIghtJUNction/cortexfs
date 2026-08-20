use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelFrameBody, ChannelRuntimeEvent,
};
use reqwest::Client;

use crate::{
    api,
    config::Config,
    error::{Error, Result},
    socket::Session,
};

pub(super) async fn handle(
    client: &Client,
    config: &Config,
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
            let receipt = api::send_message(client, config, message).await?;
            session.receipt(request_id, receipt)?;
        }
        ChannelFrameBody::Effect { target, effect, .. } => {
            api::effect(client, config, &target, effect).await?;
        }
        ChannelFrameBody::Command {
            request_id,
            session: session_id,
            command_id,
            command: ChannelCommand::Invoke { name, payload },
            target: Some(target),
        } => {
            super::invoke::handle(
                client, config, session, request_id, session_id, command_id, target, name, payload,
            )
            .await?;
        }
        ChannelFrameBody::Command {
            request_id,
            session: session_id,
            command_id,
            command,
            target,
        } => {
            let Some(target) = target else {
                return reject(
                    session,
                    request_id,
                    session_id,
                    command_id,
                    "Slack command target is missing",
                );
            };
            let reply = crate::socket::CommandReply {
                request_id,
                session: session_id,
                command_id,
            };
            if let Some(kind) = api::pending_kind(&command) {
                session.remember_command(crate::socket::PendingCommand {
                    reply: reply.clone(),
                    target: target.clone(),
                    kind,
                })?;
            }
            match api::send_command(client, config, &target, &reply.command_id, &command).await {
                Ok(api::CommandOutcome::Immediate(result)) => {
                    session.command_result(reply, result)?;
                }
                Ok(api::CommandOutcome::Pending(_kind)) => {}
                Err(_error) => {
                    session.remove_command(&reply.command_id);
                    session.command_result(
                        reply,
                        ChannelCommandResult::Rejected {
                            reason: "Slack could not present the command".to_owned(),
                        },
                    )?;
                }
            }
        }
        ChannelFrameBody::Event {
            event: ChannelRuntimeEvent::Disconnected,
        } => return Err(Error::Api("channel runtime disconnected".to_owned())),
        ChannelFrameBody::Error {
            request_id: Some(request_id),
            message,
            ..
        } => {
            return Err(Error::Api(format!(
                "runtime rejected {request_id}: {message}"
            )));
        }
        _ => {}
    }
    Ok(())
}

fn reject(
    session: &Session,
    request_id: String,
    session_id: String,
    command_id: String,
    reason: &str,
) -> Result<()> {
    session.command_result(
        crate::socket::CommandReply {
            request_id,
            session: session_id,
            command_id,
        },
        ChannelCommandResult::Rejected {
            reason: reason.to_owned(),
        },
    )
}
