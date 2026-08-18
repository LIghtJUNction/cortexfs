use cortexfs_runtime_client::interaction::{
    InteractionEvent, InteractionFrame, InteractionOrigin, InteractionPayload, InteractionRequest,
    InteractionResult, interaction_event_from_agent_frame,
};

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    use cortexfs_runtime_client::session;

    #[test]
    fn interaction_request_round_trips_with_origin() -> Result<(), Box<dyn std::error::Error>> {
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
        let decoded = InteractionFrame::decode(&frame.encode()?)?;
        assert_eq!(decoded, frame);
        Ok(())
    }

    #[test]
    fn interaction_events_normalize_existing_agent_frames() {
        let event = interaction_event_from_agent_frame(
            "channel-1",
            r#"{"type":"delta","run":"run-1","text":"hi"}"#,
        );
        assert_eq!(
            event,
            Some(InteractionEvent::Delta {
                request_id: "channel-1".to_owned(),
                run: "run-1".to_owned(),
                text: "hi".to_owned(),
            })
        );
    }

    #[test]
    fn interaction_frame_rejects_unknown_abi() {
        let frame = InteractionFrame {
            abi: "other".to_owned(),
            payload: InteractionPayload::Event(InteractionEvent::Done {
                request_id: "r".to_owned(),
                run: "run".to_owned(),
                status: "ok".to_owned(),
            }),
        };
        assert!(frame.encode().is_err());
    }

    #[test]
    fn interaction_request_uses_the_agent_socket_wire() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> Result<(), std::io::Error> {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            let value: serde_json::Value =
                serde_json::from_str(&line).map_err(std::io::Error::other)?;
            if value.get("abi").and_then(serde_json::Value::as_str)
                != Some("cortexfs.interaction/v1")
            {
                return Err(std::io::Error::other("missing interaction ABI"));
            }
            stream.write_all(b"{\"type\":\"done\",\"status\":\"ok\"}\n")
        });
        let request = InteractionRequest::input(
            "terminal-1",
            "session-1",
            "hello",
            InteractionOrigin {
                transport: "terminal".to_owned(),
                ..InteractionOrigin::default()
            },
        );
        let mut frames = Vec::new();
        session::send_interaction_stream(&socket, request, |frame| {
            frames.push(frame.to_owned());
            Ok::<(), cortexfs_runtime_client::RuntimeClientError>(())
        })?;
        server
            .join()
            .map_err(|_error| std::io::Error::other("server panicked"))??;
        assert_eq!(frames, [r#"{"type":"done","status":"ok"}"#]);
        Ok(())
    }

    #[test]
    fn interaction_events_normalize_the_agent_stream() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> Result<(), std::io::Error> {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            stream.write_all(
                b"{\"type\":\"start\",\"run\":\"run-1\"}\n{\"type\":\"delta\",\"run\":\"run-1\",\"text\":\"ok\"}\n{\"type\":\"done\",\"run\":\"run-1\",\"status\":\"ok\"}\n",
            )
        });
        let request = InteractionRequest::input(
            "typed-1",
            "session-1",
            "hello",
            InteractionOrigin {
                transport: "terminal".to_owned(),
                ..InteractionOrigin::default()
            },
        );
        let mut events = Vec::new();
        session::send_interaction_events(&socket, request, |event| {
            events.push(event);
            Ok::<(), cortexfs_runtime_client::RuntimeClientError>(())
        })?;
        server
            .join()
            .map_err(|_error| std::io::Error::other("server panicked"))??;
        assert!(
            matches!(events.first(), Some(InteractionEvent::Started { request_id, .. }) if request_id == "typed-1")
        );
        assert!(
            matches!(events.get(1), Some(InteractionEvent::Delta { text, .. }) if text == "ok")
        );
        assert!(
            matches!(events.last(), Some(InteractionEvent::Done { status, .. }) if status == "ok")
        );
        Ok(())
    }

    #[test]
    fn interaction_events_can_answer_a_runtime_command() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || -> Result<(), std::io::Error> {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            stream.write_all(
                b"{\"type\":\"approval_request\",\"run\":\"run-1\",\"id\":\"call-1\",\"name\":\"example.echo\",\"args\":[]}\n",
            )?;
            stream.flush()?;
            line.clear();
            BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            let frame: InteractionFrame =
                serde_json::from_str(&line).map_err(std::io::Error::other)?;
            assert!(matches!(
                frame.payload,
                InteractionPayload::Request(InteractionRequest::CommandResult {
                    command_id,
                    result: InteractionResult::Accepted,
                    ..
                }) if command_id == "call-1"
            ));
            stream.write_all(b"{\"type\":\"done\",\"run\":\"run-1\",\"status\":\"ok\"}\n")
        });
        let request = InteractionRequest::input(
            "typed-commands",
            "session-1",
            "hello",
            InteractionOrigin {
                transport: "terminal".to_owned(),
                ..InteractionOrigin::default()
            },
        );
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
        server
            .join()
            .map_err(|_error| std::io::Error::other("server panicked"))??;
        assert!(matches!(
            events.first(),
            Some(InteractionEvent::Command { .. })
        ));
        assert!(matches!(events.last(), Some(InteractionEvent::Done { .. })));
        Ok(())
    }
}
