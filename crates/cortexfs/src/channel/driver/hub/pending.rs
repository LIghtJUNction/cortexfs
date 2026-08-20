use std::{collections::btree_map::Entry, sync::mpsc, time::Duration};

use cortexfs_channels::ChannelCommandResult;

use super::super::DriverError;
use super::DriverHub;

const COMMAND_TIMEOUT: Duration = Duration::from_mins(2);

impl DriverHub {
    pub(crate) fn complete_command(
        &self,
        request_id: &str,
        command_id: &str,
        result: ChannelCommandResult,
    ) -> bool {
        let Ok(mut commands) = self.commands.lock() else {
            return false;
        };
        let Some(entry) = commands.get(request_id) else {
            return false;
        };
        if entry.0 != command_id {
            return false;
        }
        commands
            .remove(request_id)
            .is_some_and(|(_id, sender)| sender.send(result).is_ok())
    }

    pub(super) fn register_command(
        &self,
        request_id: &str,
        command_id: &str,
    ) -> Result<mpsc::Receiver<ChannelCommandResult>, DriverError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut commands = self.commands.lock().map_err(|_error| DriverError::Lock)?;
        match commands.entry(request_id.to_owned()) {
            Entry::Vacant(entry) => {
                entry.insert((command_id.to_owned(), sender));
                Ok(receiver)
            }
            Entry::Occupied(_) => Err(DriverError::Rejected),
        }
    }

    pub(super) fn wait_command(
        &self,
        request_id: &str,
        receiver: &mpsc::Receiver<ChannelCommandResult>,
    ) -> Result<(), DriverError> {
        match receiver.recv_timeout(COMMAND_TIMEOUT) {
            Ok(ChannelCommandResult::Accepted | ChannelCommandResult::Value { .. }) => Ok(()),
            Ok(ChannelCommandResult::Rejected { .. }) => Err(DriverError::Rejected),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.forget_command(request_id);
                Err(DriverError::CommandTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(DriverError::Unavailable),
        }
    }

    pub(super) fn forget_command(&self, request_id: &str) {
        if let Ok(mut commands) = self.commands.lock() {
            let _ignored = commands.remove(request_id);
        }
    }
}
