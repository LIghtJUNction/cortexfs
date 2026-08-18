#![expect(
    clippy::redundant_pub_crate,
    reason = "socket helper is private driver plumbing"
)]

use cortexfs_channels::{
    ChannelActions, ChannelCapabilities, ChannelCommandResult, ChannelDriverSession,
    ChannelFrameBody, ChannelId, ChannelIncoming, DeliveryReceipt, MessageTarget,
};
use serde_json::Value;

use crate::error::{Error, Result};

mod command;
pub(crate) use command::{CommandReply, PendingCommand, PendingKind};

#[derive(Clone)]
pub(crate) struct Session {
    client: ChannelDriverSession,
    commands: command::State,
}

impl Session {
    pub(crate) async fn connect(config: &crate::config::Config) -> Result<Self> {
        let path = config.socket.clone();
        let timeout = config.reply_timeout;
        tokio::task::spawn_blocking(move || {
            Ok(Self {
                client: ChannelDriverSession::connect_retry(
                    &path,
                    &ChannelId::from_static("slack"),
                    ChannelCapabilities {
                        group: true,
                        threads: true,
                        reactions: true,
                        attachments: true,
                        receive_attachments: true,
                        send_attachments: true,
                        commands: true,
                        choices: true,
                        websocket: true,
                        ..ChannelCapabilities::text()
                    },
                    ChannelActions {
                        reaction: true,
                        edit: true,
                        delete: true,
                        pin: true,
                        unpin: true,
                        redact: true,
                        ..ChannelActions::empty()
                    },
                    "slack",
                    timeout,
                )?,
                commands: command::State::default(),
            })
        })
        .await
        .map_err(|error| Error::Task(error.to_string()))?
    }

    pub(crate) fn send_incoming(&self, incoming: ChannelIncoming) -> Result<()> {
        self.client.send_incoming(incoming)?;
        Ok(())
    }

    pub(crate) fn next(&self) -> Result<ChannelFrameBody> {
        Ok(self.client.recv()?)
    }

    pub(crate) fn receipt(&self, request_id: String, receipt: DeliveryReceipt) -> Result<()> {
        self.client.send_receipt(request_id, receipt)?;
        Ok(())
    }

    pub(crate) fn send_frame(&self, frame: ChannelFrameBody) -> Result<()> {
        self.client.send_frame(frame)?;
        Ok(())
    }

    pub(crate) fn remember_command(&self, pending: PendingCommand) -> Result<()> {
        self.commands
            .insert(pending)
            .then_some(())
            .ok_or_else(|| Error::Api("Slack command state unavailable".to_owned()))
    }

    pub(crate) fn remove_command(&self, command_id: &str) {
        self.commands.remove(command_id);
    }

    pub(crate) fn take_input(&self, target: &MessageTarget) -> Option<CommandReply> {
        self.commands.take_input(target)
    }

    pub(crate) fn take_action(
        &self,
        payload: &Value,
    ) -> Option<(CommandReply, ChannelCommandResult)> {
        self.commands.take_action(payload)
    }

    pub(crate) fn command_result(
        &self,
        reply: CommandReply,
        result: ChannelCommandResult,
    ) -> Result<()> {
        self.send_frame(ChannelFrameBody::CommandResult {
            request_id: reply.request_id,
            session: reply.session,
            command_id: reply.command_id,
            result,
        })
    }
}
