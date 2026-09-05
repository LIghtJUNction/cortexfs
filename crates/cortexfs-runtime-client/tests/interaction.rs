use cortexfs_runtime_client::interaction::{
    AttachmentMode, INTERACTION_V2_ABI, InteractionCapability, InteractionCorrelation,
    InteractionEvent, InteractionFrame, InteractionOrigin, InteractionPayload, InteractionRequest,
    InteractionResult, InteractionSide, InteractionV2Error, InteractionV2Event, InteractionV2Frame,
    InteractionV2Kind, interaction_event_from_agent_frame,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::thread::{self, JoinHandle};

    use cortexfs_runtime_client::session;
    use tempfile::TempDir;

    type SocketServer = (TempDir, PathBuf, JoinHandle<io::Result<()>>);

    fn socket_server(
        handler: impl FnOnce(&mut UnixStream, &str) -> io::Result<()> + Send + 'static,
    ) -> io::Result<SocketServer> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            handler(&mut stream, &line)
        });
        Ok((root, socket, task))
    }

    fn terminal_input(request_id: &str) -> InteractionRequest {
        InteractionRequest::input(
            request_id,
            "session-1",
            "hello",
            InteractionOrigin {
                transport: "terminal".to_owned(),
                ..InteractionOrigin::default()
            },
        )
    }

    fn join_server(server: JoinHandle<io::Result<()>>) -> io::Result<()> {
        server
            .join()
            .map_err(|_error| io::Error::other("server panicked"))?
    }

    #[test]
    fn interaction_v1_round_trips_and_normalizes() -> Result<(), Box<dyn std::error::Error>> {
        let request = InteractionRequest::input(
            "web-1",
            "session-1",
            "hello",
            InteractionOrigin {
                transport: "web".to_owned(),
                endpoint: Some("chat".to_owned()),
                ..InteractionOrigin::default()
            },
        );
        let frame = InteractionFrame::request(request);
        assert_eq!(InteractionFrame::decode(&frame.encode()?)?, frame);
        assert_eq!(
            interaction_event_from_agent_frame(
                "channel-1",
                r#"{"type":"delta","run":"run-1","text":"hi"}"#,
            ),
            Some(InteractionEvent::Delta {
                request_id: "channel-1".to_owned(),
                run: "run-1".to_owned(),
                text: "hi".to_owned(),
            })
        );
        Ok(())
    }

    #[test]
    fn interaction_stream_preserves_wire_events_and_commands()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_root, socket, server) = socket_server(|stream, line| {
            let value: serde_json::Value = serde_json::from_str(line).map_err(io::Error::other)?;
            if value.get("abi").and_then(serde_json::Value::as_str)
                != Some("cortexfs.interaction/v1")
            {
                return Err(io::Error::other("missing interaction ABI"));
            }
            stream.write_all(b"{\"type\":\"done\",\"status\":\"ok\"}\n")
        })?;
        let request = terminal_input("terminal-1");
        let mut frames = Vec::new();
        session::send_interaction_stream(&socket, request, |frame| {
            frames.push(frame.to_owned());
            Ok::<(), cortexfs_runtime_client::RuntimeClientError>(())
        })?;
        join_server(server)?;
        assert_eq!(frames, [r#"{"type":"done","status":"ok"}"#]);
        let (_root, socket, server) = socket_server(|stream, _line| {
            stream.write_all(
                b"{\"type\":\"start\",\"run\":\"run-1\"}\n{\"type\":\"delta\",\"run\":\"run-1\",\"text\":\"ok\"}\n{\"type\":\"done\",\"run\":\"run-1\",\"status\":\"ok\"}\n",
            )
        })?;
        let request = terminal_input("typed-1");
        let mut events = Vec::new();
        session::send_interaction_events(&socket, request, |event| {
            events.push(event);
            Ok::<(), cortexfs_runtime_client::RuntimeClientError>(())
        })?;
        join_server(server)?;
        assert!(
            matches!(events.first(), Some(InteractionEvent::Started { request_id, .. }) if request_id == "typed-1")
        );
        assert!(
            matches!(events.get(1), Some(InteractionEvent::Delta { text, .. }) if text == "ok")
        );
        assert!(
            matches!(events.last(), Some(InteractionEvent::Done { status, .. }) if status == "ok")
        );
        let (_root, socket, server) = socket_server(|stream, _line| {
            stream.write_all(
                b"{\"type\":\"approval_request\",\"run\":\"run-1\",\"id\":\"call-1\",\"name\":\"example.echo\",\"args\":[]}\n",
            )?;
            stream.flush()?;
            let mut result = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut result)?;
            let frame: InteractionFrame =
                serde_json::from_str(&result).map_err(io::Error::other)?;
            assert!(matches!(
                frame.payload,
                InteractionPayload::Request(InteractionRequest::CommandResult {
                    command_id,
                    result: InteractionResult::Accepted,
                    ..
                }) if command_id == "call-1"
            ));
            stream.write_all(b"{\"type\":\"done\",\"run\":\"run-1\",\"status\":\"ok\"}\n")
        })?;
        let request = terminal_input("typed-commands");
        let mut events = Vec::new();
        session::send_interaction_events_with_commands(
            &socket,
            request,
            |event| {
                events.push(event);
                Ok::<(), cortexfs_runtime_client::RuntimeClientError>(())
            },
            |_event| Ok(InteractionResult::Accepted),
        )?;
        join_server(server)?;
        assert!(matches!(
            events.first(),
            Some(InteractionEvent::Command { .. })
        ));
        assert!(matches!(events.last(), Some(InteractionEvent::Done { .. })));
        Ok(())
    }

    fn durable_v2() -> InteractionV2Frame {
        InteractionV2Frame {
            abi: INTERACTION_V2_ABI.to_owned(),
            correlation: InteractionCorrelation {
                connection: "c1".into(),
                attachment: Some("a1".into()),
                request: Some("q1".into()),
                run: Some("r1".into()),
                command: None,
            },
            session_seq: Some(1),
            event: InteractionV2Event {
                side: InteractionSide::Slave,
                kind: InteractionV2Kind::Event,
                capabilities: vec![],
                mode: None,
                session: None,
                origin: None,
                durable: true,
                data: serde_json::json!({"text": "ok"}),
            },
        }
    }

    fn reject(change: impl FnOnce(&mut InteractionV2Frame), error: InteractionV2Error) {
        let mut frame = durable_v2();
        change(&mut frame);
        assert_eq!(frame.validate(), Err(error));
    }

    #[test]
    fn interaction_v2_round_trips_without_changing_v1() -> Result<(), serde_json::Error> {
        let mut v1 = InteractionFrame::event(InteractionEvent::Done {
            request_id: "q1".into(),
            run: "r1".into(),
            status: "ok".into(),
        });
        assert_eq!(
            serde_json::to_value(&v1)?,
            serde_json::json!({"abi":"cortexfs.interaction/v1","payload":{"direction":"event",
                "value":{"type":"done","request_id":"q1","run":"r1","status":"ok"}}})
        );
        v1.abi = "other".into();
        assert!(v1.encode().is_err());
        let frame = durable_v2();
        assert_eq!(frame.validate(), Ok(()));
        let decoded: InteractionV2Frame = serde_json::from_slice(&serde_json::to_vec(&frame)?)?;
        assert_eq!(decoded, frame);
        Ok(())
    }

    #[test]
    fn interaction_v2_validation_rejects_invalid_states() {
        reject(|f| f.abi = "other".into(), InteractionV2Error::WrongAbi);
        reject(
            |f| f.correlation.connection = "bad\n".into(),
            InteractionV2Error::InvalidIdentifier("connection_id"),
        );
        reject(
            |f| f.session_seq = Some(0),
            InteractionV2Error::InvalidSequence,
        );
        reject(
            |f| f.correlation.attachment = None,
            InteractionV2Error::MissingCorrelation("attachment_id"),
        );
        reject(
            |f| f.session_seq = None,
            InteractionV2Error::DurableSequenceMismatch,
        );
        let mut frame = durable_v2();
        frame.event.kind = InteractionV2Kind::Attached;
        frame.event.durable = false;
        frame.session_seq = None;
        frame.correlation.request = None;
        frame.correlation.run = None;
        frame.event.session = Some("s1".into());
        frame.event.mode = Some(AttachmentMode::Observe);
        frame.event.capabilities = vec![InteractionCapability::Observe];
        assert!(frame.validate().is_ok());
        frame.event.capabilities.push(InteractionCapability::Input);
        assert_eq!(
            frame.validate(),
            Err(InteractionV2Error::InvalidAttachmentMode)
        );
    }
}
