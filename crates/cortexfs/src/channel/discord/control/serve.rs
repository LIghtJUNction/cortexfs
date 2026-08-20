use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelDriverSession, ChannelFrameBody,
};
use reqwest::blocking::Client;

use super::super::{DiscordConfig, DiscordError, invoke};

pub(super) fn run(
    session: &ChannelDriverSession,
    client: &Client,
    config: &DiscordConfig,
) -> Result<(), DiscordError> {
    loop {
        match session.recv()? {
            ChannelFrameBody::Command {
                request_id,
                session: run,
                command_id,
                command: ChannelCommand::Invoke { name, payload },
                target: Some(target),
            } => {
                let result =
                    match invoke::run(client, config, &target, &command_id, &name, &payload) {
                        Ok(payload) => ChannelCommandResult::Value { payload },
                        Err(error) => ChannelCommandResult::Rejected {
                            reason: error.to_string(),
                        },
                    };
                session.send_frame(ChannelFrameBody::CommandResult {
                    request_id,
                    session: run,
                    command_id,
                    result,
                })?;
            }
            ChannelFrameBody::Command {
                request_id,
                session: run,
                command_id,
                ..
            } => session.send_frame(ChannelFrameBody::CommandResult {
                request_id,
                session: run,
                command_id,
                result: ChannelCommandResult::Rejected {
                    reason: "Discord control command is unsupported".to_owned(),
                },
            })?,
            ChannelFrameBody::Error { .. } => {
                return Err(DiscordError::Protocol("Discord control failed".to_owned()));
            }
            _ => {}
        }
    }
}
