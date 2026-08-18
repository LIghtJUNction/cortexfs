use std::{
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc::SyncSender},
};

use cortexfs_channels::{ChannelActions, ChannelIncomingEvent, InboundMessage, OutboundMessage};

use super::{DriverConfig, driverprogress, session::SessionEvent};

pub(super) struct WorkerDone {
    pub(super) event_id: String,
    pub(super) message: Option<OutboundMessage>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "worker construction passes isolated socket and routing state explicitly"
)]
pub(super) fn spawn(
    message: InboundMessage,
    config: &DriverConfig,
    writer: Arc<Mutex<UnixStream>>,
    commands: driverprogress::CommandBroker,
    actions: ChannelActions,
    commands_enabled: bool,
    events: SyncSender<SessionEvent>,
) -> std::thread::JoinHandle<()> {
    let bridge = config.bridge.clone();
    let event_id = message.id.clone();
    let target = message.target.clone();
    let session = bridge.session_for(&target);
    std::thread::spawn(move || {
        let mut progress = driverprogress::DriverProgress::shared(
            writer,
            commands,
            target,
            event_id.clone(),
            session,
            actions,
            commands_enabled,
        );
        let message = bridge.handle_with_progress(message, &mut progress).ok();
        let _ignored = events.send(SessionEvent::Done(WorkerDone { event_id, message }));
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "event worker construction passes isolated socket and routing state explicitly"
)]
pub(super) fn spawn_event(
    event_id: String,
    event: ChannelIncomingEvent,
    config: &DriverConfig,
    writer: Arc<Mutex<UnixStream>>,
    commands: driverprogress::CommandBroker,
    actions: ChannelActions,
    commands_enabled: bool,
    events: SyncSender<SessionEvent>,
) -> std::thread::JoinHandle<()> {
    let bridge = config.bridge.clone();
    let target = event.context().target.clone();
    let session = bridge.session_for(&target);
    std::thread::spawn(move || {
        let mut progress = driverprogress::DriverProgress::shared(
            writer,
            commands,
            target,
            event_id.clone(),
            session,
            actions,
            commands_enabled,
        );
        let message = bridge
            .handle_event_with_progress(&event_id, &event, &mut progress)
            .ok();
        let _ignored = events.send(SessionEvent::Done(WorkerDone { event_id, message }));
    })
}
