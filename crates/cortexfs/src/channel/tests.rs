use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;

use cortexfs_channels::{
    Attachment, ChannelActions, ChannelCapabilities, ChannelCommandResult, ChannelControlAction,
    ChannelEffect, ChannelError, ChannelEventContext, ChannelFrame, ChannelFrameBody, ChannelId,
    ChannelIncomingEvent, ChannelRuntimeEvent, ChannelSessionRoute, ConversationId,
    DeliveryReceipt, InboundMessage, MessageBody, MessageTarget, Participant,
};

use super::bridge::{AgentChannelBridge, ChannelBridgeError, ChannelProgressSink};
use super::{
    driver::{DriverConfig, DriverHub},
    driverhandle,
};

#[derive(Default)]
struct ProgressProbe {
    starts: usize,
    deltas: Vec<String>,
    completed: Option<String>,
    error: Option<String>,
}

impl ChannelProgressSink for ProgressProbe {
    fn begin(&mut self, _inbound: &InboundMessage) {
        self.starts += 1;
    }

    fn begin_event(&mut self, _target: &MessageTarget) {
        self.starts += 1;
    }

    fn delta(&mut self, text: &str) {
        self.deltas.push(text.to_owned());
    }

    fn complete(&mut self, text: &str) {
        self.completed = Some(text.to_owned());
    }

    fn error(&mut self, text: &str) {
        self.error = Some(text.to_owned());
    }
}

fn target(channel: &str, conversation: &str) -> Result<MessageTarget, ChannelError> {
    Ok(MessageTarget {
        channel: ChannelId::new(channel)?,
        conversation: ConversationId::new(conversation)?,
        thread: None,
        reply_to: None,
    })
}

fn message(
    id: &str,
    target: MessageTarget,
    sender: &str,
    text: &str,
) -> Result<InboundMessage, ChannelError> {
    Ok(InboundMessage {
        id: id.to_owned(),
        target,
        sender: Participant {
            id: sender.to_owned(),
            ..Participant::default()
        },
        body: MessageBody::text(text)?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    })
}

fn bridge(socket: impl Into<PathBuf>) -> Result<AgentChannelBridge, ChannelError> {
    Ok(AgentChannelBridge::new(
        socket,
        ChannelSessionRoute::new("executor", "im")?
            .with_allowed_senders(["user-1", "user-2", "user-event"].map(str::to_owned)),
        None,
    ))
}

fn driver(root: &Path, channel: ChannelId, bridge: AgentChannelBridge) -> DriverConfig {
    DriverConfig {
        socket: root.join("channel.sock"),
        channel,
        bridge,
        hub: DriverHub::default(),
    }
}

fn read_line(reader: &mut impl BufRead) -> io::Result<String> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line)
}

fn reply_once(
    socket: &Path,
    reply: &'static [u8],
) -> io::Result<thread::JoinHandle<io::Result<String>>> {
    let listener = UnixListener::bind(socket)?;
    Ok(thread::spawn(move || {
        let (mut stream, _) = listener.accept()?;
        let request = read_line(&mut BufReader::new(&mut stream))?;
        stream.write_all(reply)?;
        Ok(request)
    }))
}

#[test]
fn bridge_reuses_socket_sessions_and_returns_assistant_text()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let socket = root.path().join("agent.sock");
    let listener = UnixListener::bind(&socket)?;
    let server = thread::spawn(move || -> io::Result<()> {
        let mut previous_session = None;
        for _ in 0..2 {
            let (mut stream, _) = listener.accept()?;
            let frame = read_line(&mut BufReader::new(&mut stream))?;
            let value: Value = serde_json::from_str(&frame)
                .map_err(|_error| io::Error::other("invalid agent session frame"))?;
            let attachment_seen = value
                .pointer("/payload/value/event/attachments/0/url")
                .is_some_and(|url| url == "https://example.test/image.png");
            let channel_seen = value
                .pointer("/payload/value/origin/endpoint")
                .is_some_and(|channel| channel == "telegram.primary");
            if !frame.contains("\"abi\":\"cortexfs.interaction/v1\"")
                || !frame.contains("\"transport\":\"channel\"")
                || !attachment_seen
                || !channel_seen
            {
                return Err(io::Error::other("invalid agent session frame"));
            }
            let session = value.pointer("/payload/value/session").cloned();
            if session.is_none() || session == previous_session {
                return Err(io::Error::other("authorized senders share a session"));
            }
            previous_session = session;
            stream.write_all(
            b"{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"pong\"}]}\n{\"type\":\"done\",\"status\":\"ok\"}\n",
        )?;
        }
        Ok(())
    });
    let bridge = AgentChannelBridge::new_with_channel(
        socket,
        ChannelSessionRoute::new("executor", "im")?
            .with_allowed_senders(["user-1", "user-2", "user-event"].map(str::to_owned))
            .with_identity_isolation(),
        None,
        ChannelId::new("telegram.primary")?,
    );
    for sender in ["user-1", "user-2"] {
        let mut inbound = message("message-1", target("telegram", "chat-1")?, sender, "ping")?;
        inbound.body = MessageBody::with_attachments(
            "ping",
            vec![Attachment {
                url: "https://example.test/image.png".to_owned(),
                name: Some("image.png".to_owned()),
                mime: Some("image/png".to_owned()),
            }],
        )?;
        let reply = bridge.handle(inbound)?;
        assert_eq!(reply.body.text, "pong");
        assert_eq!(reply.target.channel.as_str(), "telegram.primary");
        assert_eq!(reply.target.reply_to, Some("message-1".to_owned()));
    }
    server
        .join()
        .map_err(|error| format!("server panicked: {error:?}"))??;
    Ok(())
}

#[test]
fn bridge_forwards_stream_events_to_progress_sink() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let socket = root.path().join("agent.sock");
    let server = reply_once(
        &socket,
        b"{\"type\":\"delta\",\"text\":\"pong\"}\n{\"type\":\"done\",\"status\":\"ok\"}\n",
    )?;
    let bridge = bridge(socket)?;
    let inbound = message(
        "message-2",
        target("discord", "channel-1")?,
        "user-1",
        "ping",
    )?;
    let mut probe = ProgressProbe::default();
    let reply = bridge.handle_with_progress(inbound, &mut probe)?;
    server
        .join()
        .map_err(|error| format!("server panicked: {error:?}"))??;
    assert_eq!(probe.starts, 1);
    assert_eq!(probe.deltas, ["pong"]);
    assert_eq!(probe.completed.as_deref(), Some("pong"));
    assert_eq!(reply.body.text, "pong");
    Ok(())
}

#[test]
fn bridge_returns_safe_progress_error_without_provider_details()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let socket = root.path().join("agent.sock");
    let server = reply_once(&socket,
            b"{\"type\":\"delta\",\"text\":\"sensitive partial output\"}\n{\"type\":\"error\",\"recoverable\":false,\"message\":\"sk-secret-provider-detail\"}\n{\"type\":\"done\",\"status\":\"error\"}\n",
    )?;
    let bridge = bridge(socket)?;
    let inbound = message(
        "message-3",
        target("discord", "channel-1")?,
        "user-1",
        "ping",
    )?;
    let mut probe = ProgressProbe::default();
    let Err(error) = bridge.handle_with_progress(inbound, &mut probe) else {
        return Err(io::Error::other("agent error was not returned").into());
    };
    server
        .join()
        .map_err(|error| format!("server panicked: {error:?}"))??;
    assert!(matches!(error, ChannelBridgeError::Agent(_)));
    let Some(message) = probe.error else {
        return Err(io::Error::other("progress error was not delivered").into());
    };
    assert!(!message.contains("sk-secret-provider-detail"));
    assert!(message.contains("model/tool loop"));
    assert!(probe.deltas.is_empty());
    Ok(())
}

#[test]
fn driver_streams_effects_before_final_delivery() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let agent_socket = root.path().join("agent.sock");
    let agent = reply_once(&agent_socket,
            b"{\"type\":\"delta\",\"run\":\"run-1\",\"text\":\"pong\"}\n{\"type\":\"done\",\"run\":\"run-1\",\"status\":\"ok\"}\n",
    )?;
    let bridge = bridge(agent_socket)?;
    let channel = ChannelId::new("telegram")?;
    let config = driver(root.path(), channel.clone(), bridge);
    let (mut runtime, mut adapter) = UnixStream::pair()?;
    let inbound = message(
        "message-4",
        target(channel.as_str(), "chat-1")?,
        "user-1",
        "ping",
    )?;
    let (response, close) = driverhandle::handle(
        ChannelFrame::new(ChannelFrameBody::Inbound {
            event_id: "event-1".to_owned(),
            message: inbound,
        }),
        &config,
        &mut runtime,
    );
    let mut reader = BufReader::new(&mut adapter);
    assert!(read_line(&mut reader)?.contains("\"typing\""));
    assert!(read_line(&mut reader)?.contains("\"preview\""));
    assert!(read_line(&mut reader)?.contains("\"active\":false"));
    assert!(!close);
    assert!(matches!(
        response.map(|frame| frame.frame),
        Some(ChannelFrameBody::Deliver { .. })
    ));
    agent
        .join()
        .map_err(|_error| io::Error::other("agent panicked"))??;
    Ok(())
}

#[test]
fn driver_routes_non_message_event_with_structured_payload()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let agent_socket = root.path().join("agent.sock");
    let agent = reply_once(&agent_socket,
        b"{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"seen\"}]}\n{\"type\":\"done\",\"status\":\"ok\"}\n",
    )?;
    let channel = ChannelId::new("telegram")?;
    let target = target(channel.as_str(), "chat-event")?;
    let config = driver(root.path(), channel, bridge(agent_socket)?);
    let (mut runtime, _adapter) = UnixStream::pair()?;
    let event = ChannelIncomingEvent::Reaction {
        context: ChannelEventContext {
            target: target.clone(),
            participant: Some(Participant {
                id: "user-event".to_owned(),
                ..Participant::default()
            }),
            timestamp_ms: None,
            metadata: std::collections::BTreeMap::new(),
        },
        message_id: "message-event".to_owned(),
        emoji: "👍".to_owned(),
        added: true,
    };
    let (response, close) = driverhandle::handle(
        ChannelFrame::new(ChannelFrameBody::InboundEvent {
            event_id: "reaction-event".to_owned(),
            event,
        }),
        &config,
        &mut runtime,
    );
    assert!(!close);
    let Some(ChannelFrame {
        frame: ChannelFrameBody::Deliver { message, .. },
        ..
    }) = response
    else {
        return Err(format!("event did not produce delivery: {response:?}").into());
    };
    assert_eq!(message.body.text, "seen");
    assert_eq!(
        message.target,
        MessageTarget {
            reply_to: Some("message-event".to_owned()),
            ..target
        }
    );
    let request = agent
        .join()
        .map_err(|error| io::Error::other(format!("agent panicked: {error:?}")))??;
    let event_seen = serde_json::from_str::<Value>(&request).is_ok_and(|value| {
        value
            .pointer("/payload/value/event/type")
            .and_then(Value::as_str)
            == Some("reaction")
    });
    if !event_seen {
        return Err(
            io::Error::other(format!("structured event was not forwarded: {request}")).into(),
        );
    }
    Ok(())
}

#[test]
fn driver_round_trips_runtime_command_on_the_channel_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let agent_socket = root.path().join("agent.sock");
    let agent_listener = UnixListener::bind(&agent_socket)?;
    let agent = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _) = agent_listener.accept()?;
        let _request = read_line(&mut BufReader::new(&mut stream))?;
        stream.write_all(
            br#"{"type":"approval_request","run":"run-2","id":"call-2","name":"fs.write","args":{}}
"#,
        )?;
        let result = read_line(&mut BufReader::new(&mut stream))?;
        if !result.contains("\"accepted\"") {
            return Err(io::Error::other("command result was not accepted"));
        }
        stream.write_all(
            b"{\"type\":\"delta\",\"run\":\"run-2\",\"text\":\"done\"}\n{\"type\":\"done\",\"run\":\"run-2\",\"status\":\"ok\"}\n",
        )?;
        Ok(())
    });
    let channel = ChannelId::new("telegram")?;
    let target = target(channel.as_str(), "chat-2")?;
    let route = ChannelSessionRoute::new("executor", "im")?
        .with_allowed_senders(["user-1", "user-2", "user-event"].map(str::to_owned));
    let bridge = AgentChannelBridge::new(agent_socket, route.clone(), None);
    let config = driver(root.path(), channel, bridge);
    let (runtime, adapter) = UnixStream::pair()?;
    let mut adapter_writer = adapter.try_clone()?;
    let server = thread::spawn(move || super::driver::serve_once(runtime, &config));
    let inbound = message("event-2", target.clone(), "user-2", "approve")?;
    let request_id = route.request_id_for(&inbound);
    let frame = ChannelFrame::new(ChannelFrameBody::Inbound {
        event_id: "event-2".to_owned(),
        message: inbound,
    });
    adapter_writer.write_all(&frame.encode()?)?;
    let mut reader = BufReader::new(adapter);
    assert!(read_line(&mut reader)?.contains("\"typing\""));
    assert!(read_line(&mut reader)?.contains("\"command\""));
    let session = route.session_for(&target);
    let result = ChannelFrame::new(ChannelFrameBody::CommandResult {
        request_id,
        session,
        command_id: "call-2".to_owned(),
        result: ChannelCommandResult::Accepted,
    });
    adapter_writer.write_all(&result.encode()?)?;
    for expected in ["preview", "\"active\":false", "deliver"] {
        let line = read_line(&mut reader)?;
        assert!(line.contains(expected), "missing {expected}: {line}");
    }
    drop(reader);
    drop(adapter_writer);
    server
        .join()
        .map_err(|_error| io::Error::other("driver panicked"))??;
    agent
        .join()
        .map_err(|_error| io::Error::other("agent panicked"))??;
    Ok(())
}

#[test]
fn driver_accepts_unsolicited_status_and_receipt_frames() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let channel = ChannelId::new("telegram")?;
    let config = driver(
        root.path(),
        channel.clone(),
        bridge(root.path().join("agent.sock"))?,
    );
    let (_runtime, mut adapter) = UnixStream::pair()?;
    for frame in [
        ChannelFrame::new(ChannelFrameBody::Event {
            event: ChannelRuntimeEvent::Heartbeat,
        }),
        ChannelFrame::new(ChannelFrameBody::Receipt {
            request_id: "send-1".to_owned(),
            receipt: DeliveryReceipt {
                channel,
                message_id: "message-1".to_owned(),
                target: target("telegram", "chat-1")?,
                timestamp_ms: None,
            },
        }),
        ChannelFrame::new(ChannelFrameBody::HealthRequest {
            request_id: "health-1".to_owned(),
        }),
    ] {
        let (response, close) = driverhandle::handle(frame, &config, &mut adapter);
        assert!(!close);
        if let Some(response) = response {
            assert!(matches!(
                response.frame,
                ChannelFrameBody::HealthResponse { ref request_id, .. }
                    if request_id == "health-1"
            ));
        }
    }
    Ok(())
}

#[test]
fn driver_control_request_reaches_the_registered_adapter() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let channel = ChannelId::new("discord")?;
    let adapter_config = driver(
        root.path(),
        channel.clone(),
        bridge(root.path().join("agent.sock"))?,
    );
    let control_config = DriverConfig {
        socket: root.path().join("control.sock"),
        ..adapter_config.clone()
    };
    let (mut adapter, adapter_runtime) = UnixStream::pair()?;
    let adapter_thread =
        thread::spawn(move || super::driver::serve_once(adapter_runtime, &adapter_config));
    adapter.write_all(
        &ChannelFrame::new(ChannelFrameBody::Hello {
            request_id: "adapter-hello".to_owned(),
            channel: channel.clone(),
            capabilities: ChannelCapabilities::text(),
            actions: ChannelActions {
                typing: true,
                ..ChannelActions::empty()
            },
        })
        .encode()?,
    )?;
    adapter.write_all(
        &ChannelFrame::new(ChannelFrameBody::Start {
            request_id: "adapter-start".to_owned(),
        })
        .encode()?,
    )?;
    let mut adapter_reader = BufReader::new(adapter.try_clone()?);
    assert!(read_line(&mut adapter_reader)?.contains("connected"));
    assert!(read_line(&mut adapter_reader)?.contains("connected"));

    let (mut control, control_runtime) = UnixStream::pair()?;
    let control_thread =
        thread::spawn(move || super::driver::serve_once(control_runtime, &control_config));
    control.write_all(
        &ChannelFrame::new(ChannelFrameBody::ControlHello {
            request_id: "control-hello".to_owned(),
            channel: channel.clone(),
        })
        .encode()?,
    )?;
    let mut control_reader = BufReader::new(control.try_clone()?);
    assert!(read_line(&mut control_reader)?.contains("connected"));
    let target = target(channel.as_str(), "room-1")?;
    control.write_all(
        &ChannelFrame::new(ChannelFrameBody::ControlRequest {
            request_id: "control-1".to_owned(),
            action: ChannelControlAction::Effect {
                target,
                effect: ChannelEffect::Typing { active: true },
            },
        })
        .encode()?,
    )?;
    let line = read_line(&mut control_reader)?;
    assert!(line.contains("control_response"));
    assert!(line.contains("\"accepted\":true"));
    let line = read_line(&mut adapter_reader)?;
    assert!(line.contains("\"effect\""), "adapter frame: {line}");
    drop(control_reader);
    drop(control);
    control_thread
        .join()
        .map_err(|error| io::Error::other(format!("control driver panicked: {error:?}")))??;
    drop(adapter_reader);
    drop(adapter);
    adapter_thread
        .join()
        .map_err(|error| io::Error::other(format!("adapter driver panicked: {error:?}")))??;
    Ok(())
}

#[test]
fn bridge_answers_slash_help_without_an_agent_socket() -> Result<(), Box<dyn std::error::Error>> {
    let bridge = bridge("/tmp/missing-agent.sock")?;
    let reply = bridge.handle(message(
        "help-1",
        target("telegram", "dm-1")?,
        "user-1",
        "/help",
    )?)?;
    assert!(reply.body.text.contains("/models"));
    assert!(reply.body.text.contains("/new"));
    assert_eq!(reply.target.reply_to.as_deref(), Some("help-1"));
    Ok(())
}

#[test]
fn slash_new_rotates_the_derived_session() -> Result<(), Box<dyn std::error::Error>> {
    let bridge = bridge("/tmp/missing-agent.sock")?;
    let inbound = message("new-1", target("telegram", "dm")?, "user-1", "/new")?;
    let first = bridge.handle(inbound.clone())?;
    assert!(first.body.text.contains("-1"));
    let second = bridge.handle(inbound)?;
    assert!(second.body.text.contains("-2"));
    Ok(())
}

#[test]
fn unauthorized_senders_never_reach_agent_or_progress() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let socket = root.path().join("agent.sock");
    for allowed in [vec![], vec!["trusted".to_owned()]] {
        let route = ChannelSessionRoute::new("executor", "im")?
            .with_identity_isolation()
            .with_allowed_senders(allowed);
        let bridge = AgentChannelBridge::new(&socket, route, None);
        for channel in ["telegram", "discord", "slack"] {
            let mut inbound = message(
                "denied",
                target(channel, "trusted")?,
                "unknown",
                "run a tool",
            )?;
            inbound.sender.display_name = Some("trusted".to_owned());
            inbound
                .metadata
                .insert("identity".to_owned(), "trusted".to_owned());
            let mut progress = ProgressProbe::default();
            for input in ["run a tool", "/help", "/new", "/model trusted/model"] {
                inbound.body = MessageBody::text(input)?;
                assert!(matches!(
                    bridge.handle_with_progress(inbound.clone(), &mut progress),
                    Err(ChannelBridgeError::Channel(ChannelError::SenderDenied))
                ));
            }
            for participant in [None, Some(inbound.sender.clone())] {
                let event = ChannelIncomingEvent::Typing {
                    context: ChannelEventContext {
                        target: inbound.target.clone(),
                        participant,
                        timestamp_ms: None,
                        metadata: inbound.metadata.clone(),
                    },
                    active: true,
                };
                assert!(matches!(
                    bridge.handle_event_with_progress("event", &event, &mut progress),
                    Err(ChannelBridgeError::Channel(ChannelError::SenderDenied))
                ));
            }
            assert_eq!(progress.starts, 0);
            assert!(progress.completed.is_none() && progress.error.is_none());
        }
    }
    assert!(
        !socket.exists(),
        "denied requests must not create an agent endpoint"
    );
    Ok(())
}
