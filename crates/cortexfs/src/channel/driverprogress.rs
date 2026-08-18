use std::{
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
};

use cortexfs_channels::{
    ChannelActions, ChannelCommandResult, ChannelEffect, ChannelFrame, ChannelFrameBody,
    MessageTarget,
};

mod command;
mod interaction;
mod writer;

pub(super) use command::CommandBroker;

pub(super) fn new_broker() -> CommandBroker {
    CommandBroker::default()
}

pub(super) struct DriverProgress<'a> {
    writer: writer::Output<'a>,
    target: MessageTarget,
    request_id: String,
    session: String,
    commands: Option<CommandBroker>,
    actions: ChannelActions,
    commands_enabled: bool,
    text: String,
}

impl<'a> DriverProgress<'a> {
    pub(super) fn new(
        stream: &'a mut UnixStream,
        target: MessageTarget,
        request_id: String,
    ) -> Self {
        Self {
            writer: writer::Output::Borrowed(stream),
            target,
            request_id,
            session: String::new(),
            commands: None,
            actions: all_actions(),
            commands_enabled: true,
            text: String::new(),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared progress binds one socket run to its negotiated capabilities"
    )]
    pub(super) fn shared(
        stream: Arc<Mutex<UnixStream>>,
        commands: CommandBroker,
        target: MessageTarget,
        request_id: String,
        session: String,
        actions: ChannelActions,
        commands_enabled: bool,
    ) -> Self {
        Self {
            writer: writer::Output::Shared(stream),
            target,
            request_id,
            session,
            commands: Some(commands),
            actions,
            commands_enabled,
            text: String::new(),
        }
    }

    fn effect(&mut self, effect: ChannelEffect) {
        if !self.actions.supports(effect.action()) {
            return;
        }
        let frame = ChannelFrame::new(ChannelFrameBody::Effect {
            request_id: self.request_id.clone(),
            target: self.target.clone(),
            effect,
        });
        self.write(&frame);
    }

    fn write(&mut self, frame: &ChannelFrame) -> bool {
        self.writer.write(frame)
    }
}

const fn all_actions() -> ChannelActions {
    ChannelActions {
        typing: true,
        preview: true,
        reaction: true,
        edit: true,
        delete: true,
        mark_read: true,
        pin: true,
        unpin: true,
        redact: true,
    }
}

pub(super) fn complete(
    commands: &CommandBroker,
    request_id: &str,
    session: &str,
    command_id: &str,
    result: ChannelCommandResult,
) -> bool {
    interaction::complete(commands, request_id, session, command_id, result)
}

pub(super) fn reject_all(commands: &CommandBroker) {
    interaction::reject_all(commands);
}
