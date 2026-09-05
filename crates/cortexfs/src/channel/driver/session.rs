use std::{
    collections::BTreeMap,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex, mpsc},
};

use cortexfs_channels::{ChannelFrame, ChannelFrameBody};

use super::{
    DriverConfig, DriverError, reader,
    worker::{self, WorkerDone},
};
use crate::channel::{driverhandle, driverprogress::CommandBroker};

mod features;
mod input;
mod output;

pub(super) enum SessionEvent {
    Frame(ChannelFrame),
    Invalid,
    Closed,
    Done(WorkerDone),
}

pub(super) fn serve(stream: UnixStream, config: &DriverConfig) -> Result<(), DriverError> {
    let reader_stream = stream.try_clone()?;
    let writer = Arc::new(Mutex::new(stream));
    let mut registration = None;
    let mut handshake = false;
    let (events, incoming) = mpsc::sync_channel(64);
    let reader = reader::spawn(reader_stream, events.clone());
    let commands = CommandBroker::default();
    let mut features = features::DriverFeatures::default();
    let mut active = BTreeMap::new();
    while let Ok(event) = incoming.recv() {
        match event {
            SessionEvent::Frame(frame) => {
                if !handshake {
                    handshake = true;
                    if let &ChannelFrameBody::Hello {
                        capabilities,
                        actions,
                        ..
                    } = &frame.frame
                    {
                        registration = Some(config.hub.attach(
                            &config.channel,
                            Arc::clone(&writer),
                            capabilities,
                            actions,
                        ));
                    }
                }
                if input::handle(
                    frame,
                    config,
                    &writer,
                    &commands,
                    &events,
                    &mut active,
                    &mut features,
                )? {
                    break;
                }
            }
            SessionEvent::Invalid => output::send(&writer, &driverhandle::error(None))?,
            SessionEvent::Closed => break,
            SessionEvent::Done(done) => output::finish(done, &writer, &mut active)?,
        }
    }
    output::close(&writer, &commands);
    for worker in active.into_values() {
        let _ignored = worker.join();
    }
    let _ignored = reader.join();
    drop(registration);
    Ok(())
}
