use std::time::Duration;

use cortexfs_channels::{ChannelEffect, ChannelFrame, ChannelFrameBody};
use cortexfs_runtime_client::interaction::{InteractionEvent, InteractionResult};

use super::{DriverProgress, command};
use crate::channel::bridge::ChannelProgressSink;

const MAX_PREVIEW_BYTES: usize = 64 * 1024;

impl ChannelProgressSink for DriverProgress<'_> {
    fn begin(&mut self, _inbound: &cortexfs_channels::InboundMessage) {
        self.effect(ChannelEffect::Typing { active: true });
    }

    fn begin_event(&mut self, _target: &cortexfs_channels::MessageTarget) {
        self.effect(ChannelEffect::Typing { active: true });
    }

    fn delta(&mut self, text: &str) {
        let remaining = MAX_PREVIEW_BYTES.saturating_sub(self.text.len());
        let mut end = text.len().min(remaining);
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        if let Some(prefix) = text.get(..end) {
            self.text.push_str(prefix);
        }
        if !self.text.is_empty() {
            self.effect(ChannelEffect::Preview {
                text: self.text.clone(),
            });
        }
    }

    fn complete(&mut self, _text: &str) {
        self.effect(ChannelEffect::Typing { active: false });
    }

    fn error(&mut self, _message: &str) {
        self.effect(ChannelEffect::Typing { active: false });
    }

    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching the borrowed event keeps the runtime command payload borrowed"
    )]
    fn command(&mut self, event: &InteractionEvent) -> InteractionResult {
        if !self.commands_enabled {
            return rejected("channel driver did not advertise command replies");
        }
        let InteractionEvent::Command {
            request_id,
            command_id,
            command,
            ..
        } = event
        else {
            return rejected("invalid interactive command");
        };
        let Some(commands) = self.commands.clone() else {
            return rejected("channel driver has no command reply stream");
        };
        let Some(wait) = commands.register(request_id, &self.session, command_id) else {
            return rejected("channel command broker unavailable");
        };
        let frame = ChannelFrame::new(ChannelFrameBody::Command {
            request_id: request_id.clone(),
            session: self.session.clone(),
            command_id: command_id.clone(),
            command: command::convert_command(command),
            target: Some(self.target.clone()),
        });
        if !self.writer.write(&frame) {
            commands.remove(command_id);
            return rejected("channel command could not be delivered");
        }
        wait.recv_timeout(Duration::from_mins(1))
            .unwrap_or_else(|_error| rejected("channel command reply timed out"))
    }
}

fn rejected(reason: &str) -> InteractionResult {
    InteractionResult::Rejected {
        reason: reason.to_owned(),
    }
}
