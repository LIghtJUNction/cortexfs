use std::{
    collections::BTreeMap,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc},
};

use cortexfs_channels::{ChannelFrame, ChannelFrameBody};

use super::super::{DriverConfig, DriverError, worker};
use super::{SessionEvent, features::DriverFeatures, output};
use crate::channel::{driverhandle, driverprogress};

const MAX_INFLIGHT: usize = 4;

#[expect(
    clippy::pattern_type_mismatch,
    clippy::too_many_arguments,
    reason = "the session input boundary carries the socket state needed by each worker"
)]
pub(super) fn handle(
    frame: ChannelFrame,
    config: &DriverConfig,
    writer: &Arc<Mutex<UnixStream>>,
    commands: &driverprogress::CommandBroker,
    events: &mpsc::SyncSender<SessionEvent>,
    active: &mut BTreeMap<String, std::thread::JoinHandle<()>>,
    features: &mut DriverFeatures,
) -> Result<bool, DriverError> {
    features.observe(&frame, &config.channel);
    match &frame.frame {
        ChannelFrameBody::Inbound { event_id, message } => {
            if active.len() >= MAX_INFLIGHT || active.contains_key(event_id) {
                output::send(writer, &driverhandle::error(Some(event_id.clone())))?;
            } else {
                let worker = worker::spawn(
                    message.clone(),
                    config,
                    Arc::clone(writer),
                    commands.clone(),
                    features.actions(),
                    features.commands_enabled(),
                    events.clone(),
                );
                active.insert(event_id.clone(), worker);
            }
            Ok(false)
        }
        ChannelFrameBody::InboundEvent { event_id, event } => {
            if event.context().target.channel != config.channel
                || active.len() >= MAX_INFLIGHT
                || active.contains_key(event_id)
            {
                output::send(writer, &driverhandle::error(Some(event_id.clone())))?;
            } else {
                let worker = worker::spawn_event(
                    event_id.clone(),
                    event.clone(),
                    config,
                    Arc::clone(writer),
                    commands.clone(),
                    features.actions(),
                    features.commands_enabled(),
                    events.clone(),
                );
                active.insert(event_id.clone(), worker);
            }
            Ok(false)
        }
        ChannelFrameBody::CommandResult {
            request_id,
            session,
            command_id,
            result,
        } => {
            if !config
                .hub
                .complete_command(request_id, command_id, result.clone())
                && !driverprogress::complete(
                    commands,
                    request_id,
                    session,
                    command_id,
                    result.clone(),
                )
            {
                output::send(writer, &driverhandle::error(Some(request_id.clone())))?;
            }
            Ok(false)
        }
        _ => output::control(frame, config, writer),
    }
}
