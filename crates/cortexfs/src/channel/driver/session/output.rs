use std::{
    collections::BTreeMap,
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
};

use cortexfs_channels::{ChannelFrame, ChannelFrameBody};

use super::super::{DriverConfig, DriverError, driverprogress};
use super::worker::WorkerDone;
use crate::channel::driverhandle;

pub(super) fn control(
    frame: ChannelFrame,
    config: &DriverConfig,
    writer: &Arc<Mutex<UnixStream>>,
) -> Result<bool, DriverError> {
    let mut stream = writer.lock().map_err(|_error| lock_error())?;
    let (response, close) = driverhandle::handle(frame, config, &mut stream);
    if let Some(response) = response {
        super::super::write(&mut stream, &response)?;
    }
    Ok(close)
}

pub(super) fn finish(
    done: WorkerDone,
    writer: &Arc<Mutex<UnixStream>>,
    active: &mut BTreeMap<String, std::thread::JoinHandle<()>>,
) -> Result<(), DriverError> {
    let Some(worker) = active.remove(&done.event_id) else {
        return Ok(());
    };
    let event_id = done.event_id.clone();
    let frame = done.message.map_or_else(
        || driverhandle::error(Some(event_id.clone())),
        |message| {
            ChannelFrame::new(ChannelFrameBody::Deliver {
                request_id: event_id.clone(),
                message,
            })
        },
    );
    let result = send(writer, &frame);
    let _ignored = worker.join();
    result
}

pub(super) fn send(
    writer: &Arc<Mutex<UnixStream>>,
    frame: &ChannelFrame,
) -> Result<(), DriverError> {
    let mut stream = writer.lock().map_err(|_error| lock_error())?;
    super::super::write(&mut stream, frame)
}

pub(super) fn close(writer: &Arc<Mutex<UnixStream>>, commands: &driverprogress::CommandBroker) {
    commands.reject_all();
    let _ignored = writer.lock().map(|stream| stream.shutdown(Shutdown::Both));
}

fn lock_error() -> DriverError {
    DriverError::Io(std::io::Error::other("channel driver writer lock poisoned"))
}
