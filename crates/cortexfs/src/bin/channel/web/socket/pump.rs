use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::mpsc::{self, SyncSender};
use std::time::Duration;

use cortexfs_runtime_client::{RuntimeClientError, interaction};
use tungstenite::{Message, WebSocket};

use super::{WebConfig, WebError, frame};

mod command;
mod event;
mod worker;

pub(super) enum WorkerEvent {
    Event(interaction::InteractionEvent),
    Command {
        event: interaction::InteractionEvent,
        reply: SyncSender<interaction::InteractionResult>,
    },
    Error {
        request_id: String,
        message: &'static str,
    },
    Finished(Result<(), RuntimeClientError>),
}

pub(super) fn serve(
    mut socket: WebSocket<TcpStream>,
    config: &WebConfig,
    initial: &interaction::InteractionRequest,
) -> Result<(), WebError> {
    if matches!(
        initial,
        interaction::InteractionRequest::CommandResult { .. }
    ) {
        return Err(WebError::InvalidFrame);
    }
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(100)))?;
    let (events, incoming) = mpsc::sync_channel(64);
    let runtime = worker::runtime(config.socket.clone(), initial.clone(), events.clone());
    let (control, requests) = mpsc::sync_channel(8);
    let control_worker = worker::controls(config.socket.clone(), requests, events);
    let mut pending = None;
    let request_id = initial.request_id().to_owned();
    let session = initial.session().unwrap_or("default").to_owned();
    loop {
        while let Ok(event) = incoming.try_recv() {
            event::handle(&mut socket, event, &mut pending, &request_id, &session)?;
        }
        match socket.read() {
            Ok(Message::Ping(payload)) => socket.send(Message::Pong(payload))?,
            Ok(message) => match frame::decode_message(message) {
                Ok(Some(request)) => {
                    command::submit(&mut socket, request, &mut pending, &control)?;
                }
                Ok(None) => {}
                Err(WebError::Closed) => {
                    finish(control, control_worker, runtime, pending);
                    return Ok(());
                }
                Err(error) => return Err(error),
            },
            Err(error) if transient(&error) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                finish(control, control_worker, runtime, pending);
                return Ok(());
            }
            Err(error) => return Err(WebError::WebSocket(error)),
        }
    }
}

fn finish(
    controls: SyncSender<interaction::InteractionRequest>,
    control_worker: std::thread::JoinHandle<()>,
    runtime: std::thread::JoinHandle<()>,
    pending: Option<command::PendingCommand>,
) {
    if let Some(pending) = pending {
        pending.reject();
    }
    drop(controls);
    let _ignored = control_worker.join();
    let _ignored = runtime.join();
}

fn transient(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Io(error)
            if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
    )
}
