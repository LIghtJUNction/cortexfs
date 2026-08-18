use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    os::unix::net::UnixListener,
    thread,
};

use super::{WebConfig, authorized, handle, socket};
use cortexfs::channel::http::HttpRequest;
use cortexfs_runtime_client::interaction::{
    InteractionEvent, InteractionFrame, InteractionOrigin, InteractionPayload, InteractionRequest,
    InteractionResult,
};

#[test]
fn token_is_required_only_when_configured() {
    let request = HttpRequest {
        method: "POST".into(),
        path: "/".into(),
        headers: BTreeMap::new(),
        body: String::new(),
    };
    assert!(authorized(&request, None));
    assert!(!authorized(&request, Some("secret")));
}

#[test]
fn web_host_forwards_the_interaction_frame_without_translation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("agent.sock");
    let listener = UnixListener::bind(&socket)?;
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept()?;
        let mut line = String::new();
        BufReader::new(&mut stream).read_line(&mut line)?;
        assert!(line.contains("cortexfs.interaction/v1"));
        writeln!(stream, r#"{{"type":"done","run":"r1","status":"ok"}}"#)?;
        Ok::<(), std::io::Error>(())
    });
    let frame = InteractionFrame::request(InteractionRequest::input(
        "web-1",
        "default",
        "hello",
        InteractionOrigin {
            transport: "web".to_owned(),
            ..InteractionOrigin::default()
        },
    ));
    let request = HttpRequest {
        method: "POST".into(),
        path: "/v1/interaction".into(),
        headers: BTreeMap::new(),
        body: serde_json::to_string(&frame)?,
    };
    let response = handle(
        &WebConfig {
            socket,
            bind: "127.0.0.1:0".parse()?,
            path: "/v1/interaction".into(),
            token: None,
        },
        &request,
    );
    worker
        .join()
        .map_err(|_error| std::io::Error::other("web worker panicked"))??;
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "application/x-ndjson");
    let event: InteractionFrame = serde_json::from_str(response.body.trim_end())?;
    assert_eq!(event.abi, "cortexfs.interaction/v1");
    assert!(response.body.contains("\"type\":\"done\""));
    Ok(())
}

#[test]
fn web_host_rejects_interactive_commands_without_hanging() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("agent.sock");
    let listener = UnixListener::bind(&socket)?;
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept()?;
        let mut line = String::new();
        BufReader::new(&mut stream).read_line(&mut line)?;
        writeln!(
            stream,
            r#"{{"type":"approval_request","run":"r1","id":"c1","name":"example.echo","args":[]}}"#
        )?;
        line.clear();
        BufReader::new(stream.try_clone()?).read_line(&mut line)?;
        let response: InteractionFrame =
            serde_json::from_str(&line).map_err(std::io::Error::other)?;
        assert!(matches!(
            response.payload,
            InteractionPayload::Request(InteractionRequest::CommandResult {
                command_id,
                result: InteractionResult::Rejected { .. },
                ..
            }) if command_id == "c1"
        ));
        writeln!(stream, r#"{{"type":"done","run":"r1","status":"ok"}}"#)?;
        Ok::<(), std::io::Error>(())
    });
    let frame = InteractionFrame::request(InteractionRequest::input(
        "web-command-1",
        "default",
        "hello",
        InteractionOrigin {
            transport: "web".to_owned(),
            ..InteractionOrigin::default()
        },
    ));
    let request = HttpRequest {
        method: "POST".into(),
        path: "/v1/interaction".into(),
        headers: BTreeMap::new(),
        body: serde_json::to_string(&frame)?,
    };
    let response = handle(
        &WebConfig {
            socket,
            bind: "127.0.0.1:0".parse()?,
            path: "/v1/interaction".into(),
            token: None,
        },
        &request,
    );
    worker
        .join()
        .map_err(|_error| std::io::Error::other("web worker panicked"))??;
    assert_eq!(response.status, 200);
    assert!(response.body.contains("command"));
    Ok(())
}

#[test]
fn websocket_host_returns_command_and_accepts_reply() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let agent_socket = directory.path().join("agent.sock");
    let agent_listener = UnixListener::bind(&agent_socket)?;
    let agent = thread::spawn(move || {
        let (mut stream, _) = agent_listener.accept()?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request = String::new();
        reader.read_line(&mut request)?;
        assert!(request.contains("cortexfs.interaction/v1"));
        writeln!(
            stream,
            r#"{{"type":"approval_request","run":"r1","id":"c1","name":"tool.echo","args":{{}}}}"#
        )?;
        request.clear();
        reader.read_line(&mut request)?;
        assert!(request.contains("command_result"));
        writeln!(stream, r#"{{"type":"done","run":"r1","status":"ok"}}"#)?;
        Ok::<(), std::io::Error>(())
    });
    let tcp_listener = TcpListener::bind("127.0.0.1:0")?;
    let address = tcp_listener.local_addr()?;
    let config = WebConfig {
        socket: agent_socket,
        bind: address,
        path: "/v1/interaction".to_owned(),
        token: None,
    };
    let server = thread::spawn(move || {
        let (stream, _) = tcp_listener.accept()?;
        socket::serve(stream, &config).map_err(|error| std::io::Error::other(error.to_string()))
    });
    let (mut client, _) = tungstenite::connect(format!("ws://{address}/v1/interaction"))?;
    client.send(tungstenite::Message::text(serde_json::to_string(
        &InteractionFrame::request(InteractionRequest::input(
            "ws-1",
            "default",
            "hello",
            InteractionOrigin {
                transport: "websocket".to_owned(),
                ..InteractionOrigin::default()
            },
        )),
    )?))?;
    let command = client.read()?.into_text()?;
    let command: InteractionFrame = serde_json::from_str(&command)?;
    let InteractionPayload::Event(event) = command.payload else {
        return Err("expected command event".into());
    };
    let InteractionEvent::Command { command_id, .. } = event else {
        return Err("expected approval command".into());
    };
    client.send(tungstenite::Message::text(serde_json::to_string(
        &InteractionFrame::request(InteractionRequest::CommandResult {
            request_id: "ws-1".to_owned(),
            session: "default".to_owned(),
            command_id,
            result: InteractionResult::Accepted,
        }),
    )?))?;
    let done = client.read()?.into_text()?;
    assert!(done.contains("done"));
    client.close(None)?;
    server
        .join()
        .map_err(|_error| std::io::Error::other("websocket server panicked"))??;
    agent
        .join()
        .map_err(|_error| std::io::Error::other("agent panicked"))??;
    Ok(())
}

#[test]
fn websocket_forwards_control_requests_over_a_second_runtime_stream()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let agent_socket = directory.path().join("agent.sock");
    let agent_listener = UnixListener::bind(&agent_socket)?;
    let agent = thread::spawn(move || {
        let (mut initial, _) = agent_listener.accept()?;
        let mut line = String::new();
        BufReader::new(initial.try_clone()?).read_line(&mut line)?;
        writeln!(initial, r#"{{"type":"start","run":"r1"}}"#)?;
        let (mut control, _) = agent_listener.accept()?;
        line.clear();
        BufReader::new(control.try_clone()?).read_line(&mut line)?;
        assert!(line.contains("\"type\":\"cancel\""));
        writeln!(
            control,
            r#"{{"type":"done","run":"r1","status":"cancelled"}}"#
        )?;
        writeln!(
            initial,
            r#"{{"type":"done","run":"r1","status":"cancelled"}}"#
        )?;
        Ok::<(), std::io::Error>(())
    });
    let tcp_listener = TcpListener::bind("127.0.0.1:0")?;
    let address = tcp_listener.local_addr()?;
    let config = WebConfig {
        socket: agent_socket,
        bind: address,
        path: "/v1/interaction".to_owned(),
        token: None,
    };
    let server = thread::spawn(move || {
        let (stream, _) = tcp_listener.accept()?;
        socket::serve(stream, &config).map_err(|error| std::io::Error::other(error.to_string()))
    });
    let (mut client, _) = tungstenite::connect(format!("ws://{address}/v1/interaction"))?;
    client.send(tungstenite::Message::text(serde_json::to_string(
        &InteractionFrame::request(InteractionRequest::input(
            "ws-control-1",
            "default",
            "hello",
            InteractionOrigin {
                transport: "websocket".to_owned(),
                ..InteractionOrigin::default()
            },
        )),
    )?))?;
    let start = client.read()?.into_text()?;
    assert!(start.contains("\"type\":\"started\""));
    client.send(tungstenite::Message::text(serde_json::to_string(
        &InteractionFrame::request(InteractionRequest::Cancel {
            request_id: "ws-cancel-1".to_owned(),
            run: "r1".to_owned(),
        }),
    )?))?;
    loop {
        let frame: InteractionFrame = serde_json::from_str(&client.read()?.into_text()?)?;
        if let InteractionPayload::Event(InteractionEvent::Done {
            request_id, status, ..
        }) = frame.payload
            && request_id == "ws-cancel-1"
        {
            assert_eq!(status, "cancelled");
            break;
        }
    }
    client.close(None)?;
    server
        .join()
        .map_err(|_error| std::io::Error::other("websocket server panicked"))??;
    agent
        .join()
        .map_err(|_error| std::io::Error::other("agent panicked"))??;
    Ok(())
}
