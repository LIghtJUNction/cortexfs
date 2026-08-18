use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cortexfs_runtime_client::{RuntimeClientError, interaction, session};

use super::WorkerEvent;

pub(super) fn runtime(
    socket: std::path::PathBuf,
    request: interaction::InteractionRequest,
    events: SyncSender<WorkerEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let result = session::send_interaction_events_with_commands(
            &socket,
            request,
            |event| {
                if matches!(event, interaction::InteractionEvent::Command { .. }) {
                    return Ok(());
                }
                events
                    .try_send(WorkerEvent::Event(event))
                    .map_err(|_error| RuntimeClientError::CannotWrite)
            },
            |event| {
                let (reply, wait) = mpsc::sync_channel(1);
                events
                    .try_send(WorkerEvent::Command {
                        event: event.clone(),
                        reply,
                    })
                    .map_err(|_error| RuntimeClientError::CannotWrite)?;
                Ok(wait
                    .recv_timeout(Duration::from_mins(1))
                    .unwrap_or_else(|_error| interaction::InteractionResult::Rejected {
                        reason: "web command reply timed out".to_owned(),
                    }))
            },
        );
        let _ignored = events.try_send(WorkerEvent::Finished(result));
    })
}

pub(super) fn controls(
    socket: std::path::PathBuf,
    requests: Receiver<interaction::InteractionRequest>,
    events: SyncSender<WorkerEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(request) = requests.recv() {
            let request_id = request.request_id().to_owned();
            let result = session::send_interaction_events(&socket, request, |event| {
                events
                    .try_send(WorkerEvent::Event(event))
                    .map_err(|_error| RuntimeClientError::CannotWrite)
            });
            if let Err(error) = result {
                let _ignored = events.try_send(WorkerEvent::Error {
                    request_id,
                    message: safe_error(&error),
                });
            }
        }
    })
}

#[expect(
    clippy::pattern_type_mismatch,
    reason = "matching the borrowed error preserves the diagnostic without moving it"
)]
fn safe_error(error: &RuntimeClientError) -> &'static str {
    match error {
        RuntimeClientError::CannotConnect => "agent runtime is unavailable",
        RuntimeClientError::Rejected(_) => "agent runtime rejected the request",
        _ => "agent runtime request failed",
    }
}
