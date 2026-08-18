#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "private command state crosses the Slack socket submodules"
)]

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use cortexfs_channels::{ChannelCommandResult, MessageTarget};
use serde_json::Value;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub(crate) struct CommandReply {
    pub(crate) request_id: String,
    pub(crate) session: String,
    pub(crate) command_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PendingKind {
    Input,
    Approval,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingCommand {
    pub(crate) reply: CommandReply,
    pub(crate) target: MessageTarget,
    pub(crate) kind: PendingKind,
}

#[derive(Clone, Default)]
pub(crate) struct State {
    pending: Arc<Mutex<BTreeMap<String, PendingCommand>>>,
}

impl State {
    pub(crate) fn insert(&self, pending: PendingCommand) -> bool {
        let Ok(mut commands) = self.pending.lock() else {
            return false;
        };
        commands.insert(pending.reply.command_id.clone(), pending);
        true
    }

    pub(crate) fn remove(&self, command_id: &str) {
        if let Ok(mut commands) = self.pending.lock() {
            let _ignored = commands.remove(command_id);
        }
    }

    pub(crate) fn take_input(&self, target: &MessageTarget) -> Option<CommandReply> {
        let mut commands = self.pending.lock().ok()?;
        let command_id = commands.iter().find_map(|(id, pending)| {
            (pending.kind == PendingKind::Input
                && pending.target.channel == target.channel
                && pending.target.conversation == target.conversation
                && pending.target.thread == target.thread)
                .then_some(id.clone())
        })?;
        commands.remove(&command_id).map(|pending| pending.reply)
    }

    pub(crate) fn take_action(
        &self,
        payload: &Value,
    ) -> Option<(CommandReply, ChannelCommandResult)> {
        let action = payload.get("actions")?.as_array()?.first()?;
        if action.get("action_id").and_then(Value::as_str) != Some("cortexfs_command") {
            return None;
        }
        let encoded = action.get("value")?.as_str()?;
        let value: Value = serde_json::from_str(encoded).ok()?;
        let command_id = value.get("command_id")?.as_str()?;
        let result = match value.get("result")?.as_str()? {
            "accepted" => ChannelCommandResult::Accepted,
            "rejected" => ChannelCommandResult::Rejected {
                reason: "Slack user rejected approval".to_owned(),
            },
            _ => return None,
        };
        let mut commands = self.pending.lock().ok()?;
        if commands.get(command_id)?.kind != PendingKind::Approval {
            return None;
        }
        let pending = commands.remove(command_id)?;
        drop(commands);
        Some((pending.reply, result))
    }
}
