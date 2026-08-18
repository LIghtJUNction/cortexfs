use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, SyncSender},
};

use cortexfs_channels::{ChannelCommand, ChannelCommandResult};
use cortexfs_runtime_client::interaction::{InteractionCommand, InteractionResult};

#[derive(Clone, Default)]
#[expect(
    unreachable_pub,
    reason = "the broker type crosses private channel driver submodules only"
)]
pub struct CommandBroker {
    pending: Arc<Mutex<BTreeMap<String, Pending>>>,
}

struct Pending {
    request_id: String,
    session: String,
    reply: SyncSender<InteractionResult>,
}

impl CommandBroker {
    pub(super) fn register(
        &self,
        request_id: &str,
        session: &str,
        command_id: &str,
    ) -> Option<Receiver<InteractionResult>> {
        let (reply, result) = mpsc::sync_channel(1);
        {
            let mut pending = self.pending.lock().ok()?;
            pending.insert(
                command_id.to_owned(),
                Pending {
                    request_id: request_id.to_owned(),
                    session: session.to_owned(),
                    reply,
                },
            );
        }
        Some(result)
    }

    pub(super) fn complete(
        &self,
        request_id: &str,
        session: &str,
        command_id: &str,
        result: ChannelCommandResult,
    ) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let Some(entry) = pending.get(command_id) else {
            return false;
        };
        if entry.request_id != request_id || entry.session != session {
            return false;
        }
        let Some(entry) = pending.remove(command_id) else {
            return false;
        };
        entry.reply.send(convert_result(result)).is_ok()
    }

    pub(super) fn remove(&self, command_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            let _ignored = pending.remove(command_id);
        }
    }

    pub(super) fn reject_all(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            for (_, entry) in std::mem::take(&mut *pending) {
                let _ignored = entry.reply.send(InteractionResult::Rejected {
                    reason: "channel driver closed".to_owned(),
                });
            }
        }
    }
}

#[expect(
    clippy::pattern_type_mismatch,
    reason = "matching the borrowed runtime command preserves its payload"
)]
pub(super) fn convert_command(command: &InteractionCommand) -> ChannelCommand {
    match command {
        InteractionCommand::RequestInput { prompt } => ChannelCommand::RequestInput {
            prompt: prompt.clone(),
        },
        InteractionCommand::RequestApproval { tool, arguments } => {
            ChannelCommand::RequestApproval {
                tool: tool.clone(),
                arguments: arguments.clone(),
            }
        }
        InteractionCommand::Notify { level, text } => ChannelCommand::Notify {
            level: level.clone(),
            text: text.clone(),
        },
        InteractionCommand::Invoke { name, payload } => ChannelCommand::Invoke {
            name: name.clone(),
            payload: payload.clone(),
        },
    }
}

fn convert_result(result: ChannelCommandResult) -> InteractionResult {
    match result {
        ChannelCommandResult::Accepted => InteractionResult::Accepted,
        ChannelCommandResult::Rejected { reason } => InteractionResult::Rejected { reason },
        ChannelCommandResult::Value { payload } => InteractionResult::Value { payload },
    }
}
