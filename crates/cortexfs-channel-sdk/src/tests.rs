use std::{collections::BTreeMap, io, os::unix::net::UnixListener, thread, time::Duration};

use crate::{
    ChannelCommand, ChannelCommandResult, ChannelIncoming, ChannelRuntime, MessageBody,
    OutboundMessage,
};
use cortexfs_channels::ChannelFrameBody;

mod support;

use support::{Service, inbound, read, target, write};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[test]
fn persistent_runtime_dispatches_the_extension_surface() -> TestResult {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("channel.sock");
    let listener = UnixListener::bind(&socket)?;
    let destination = target()?;
    let server_target = destination.clone();
    let worker = thread::spawn(move || -> TestResult {
        let (mut stream, _address) = listener.accept()?;
        let mut reader = io::BufReader::new(stream.try_clone()?);
        assert!(matches!(read(&mut reader)?, ChannelFrameBody::Hello { .. }));
        assert!(matches!(read(&mut reader)?, ChannelFrameBody::Start { .. }));
        assert!(matches!(
            read(&mut reader)?,
            ChannelFrameBody::Inbound { .. }
        ));
        write(
            &mut stream,
            ChannelFrameBody::Outbound {
                request_id: "out-1".to_owned(),
                message: OutboundMessage {
                    target: server_target.clone(),
                    body: MessageBody::text("reply")?,
                    metadata: BTreeMap::new(),
                },
            },
        )?;
        assert!(matches!(
            read(&mut reader)?,
            ChannelFrameBody::Receipt { .. }
        ));
        write(
            &mut stream,
            ChannelFrameBody::Command {
                request_id: "cmd-1".to_owned(),
                session: "session-1".to_owned(),
                command_id: "command-1".to_owned(),
                command: ChannelCommand::Notify {
                    level: "info".to_owned(),
                    text: "hello".to_owned(),
                },
                target: Some(server_target),
            },
        )?;
        assert!(matches!(
            read(&mut reader)?,
            ChannelFrameBody::CommandResult {
                result: ChannelCommandResult::Accepted,
                ..
            }
        ));
        write(
            &mut stream,
            ChannelFrameBody::HealthRequest {
                request_id: "health-1".to_owned(),
            },
        )?;
        assert!(matches!(
            read(&mut reader)?,
            ChannelFrameBody::HealthResponse { .. }
        ));
        write(
            &mut stream,
            ChannelFrameBody::Stop {
                request_id: "stop-1".to_owned(),
            },
        )?;
        Ok(())
    });
    let (service, calls) = Service::new(destination);
    let runtime = ChannelRuntime::connect(&socket, service, "test", Duration::from_secs(2))?;
    runtime
        .sender()
        .send(ChannelIncoming::Message(inbound()?))?;
    runtime.run()?;
    worker
        .join()
        .map_err(|_panic| io::Error::other("mock runtime panicked"))??;
    assert_eq!(*calls.lock().map_err(|_error| io::Error::other("lock"))?, 5);
    Ok(())
}
