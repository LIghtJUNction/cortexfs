use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;

use cortexfs_channels::{
    Attachment, ChannelActions, ChannelCapabilities, ChannelCommandResult, ChannelControlAction,
    ChannelEffect, ChannelEventContext, ChannelFrame, ChannelFrameBody, ChannelId,
    ChannelIncomingEvent, ChannelRuntimeEvent, ChannelSessionRoute, ConversationId,
    DeliveryReceipt, InboundMessage, MessageBody, MessageTarget, Participant,
};

use super::bridge::AgentChannelBridge;
use super::bridge::ChannelProgressSink;
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

#[test]
fn bridge_reuses_socket_sessions_and_returns_assistant_text()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let socket = root.path().join("agent.sock");
    let listener = UnixListener::bind(&socket)?;
    let server = thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = listener.accept()?;
        let mut frame = String::new();
        BufReader::new(&mut stream).read_line(&mut frame)?;
        let attachment_seen =
            serde_json::from_str::<serde_json::Value>(&frame).is_ok_and(|value| {
                value
                    .pointer("/payload/value/event/attachments/0/url")
                    .and_then(serde_json::Value::as_str)
                    == Some("https://example.test/image.png")
            });
        let channel_seen = serde_json::from_str::<serde_json::Value>(&frame).is_ok_and(|value| {
            value
                .pointer("/payload/value/origin/endpoint")
                .and_then(serde_json::Value::as_str)
                == Some("telegram.primary")
        });
        if !frame.contains("\"abi\":\"cortexfs.interaction/v1\"")
            || !frame.contains("\"transport\":\"channel\"")
            || !attachment_seen
            || !channel_seen
        {
            return Err(std::io::Error::other("invalid agent session frame"));
        }
        stream.write_all(
            b"{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"pong\"}]}\n{\"type\":\"done\",\"status\":\"ok\"}\n",
        )
    });
    let bridge = AgentChannelBridge::new_with_channel(
        socket,
        ChannelSessionRoute::new("coder", "im")?,
        None,
        ChannelId::new("telegram.primary")?,
    );
    let reply = bridge.handle(InboundMessage {
        id: "message-1".to_owned(),
        target: MessageTarget {
            channel: ChannelId::new("telegram")?,
            conversation: ConversationId::new("chat-1")?,
            thread: None,
            reply_to: None,
        },
        sender: Participant::default(),
        body: MessageBody::with_attachments(
            "ping",
            vec![Attachment {
                url: "https://example.test/image.png".to_owned(),
                name: Some("image.png".to_owned()),
                mime: Some("image/png".to_owned()),
            }],
        )?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    })?;
    server
        .join()
        .map_err(|error| format!("server panicked: {error:?}"))??;
    assert_eq!(reply.body.text, "pong");
    assert_eq!(reply.target.channel.as_str(), "telegram.primary");
    assert_eq!(reply.target.reply_to, Some("message-1".to_owned()));
    Ok(())
}

#[test]
fn bridge_forwards_stream_events_to_progress_sink() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let socket = root.path().join("agent.sock");
    let listener = UnixListener::bind(&socket)?;
    let server = thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = listener.accept()?;
        let mut frame = String::new();
        BufReader::new(&mut stream).read_line(&mut frame)?;
        stream.write_all(
            b"{\"type\":\"delta\",\"text\":\"pong\"}\n{\"type\":\"done\",\"status\":\"ok\"}\n",
        )
    });
    let bridge = AgentChannelBridge::new(socket, ChannelSessionRoute::new("coder", "im")?, None);
    let inbound = InboundMessage {
        id: "message-2".to_owned(),
        target: MessageTarget {
            channel: ChannelId::new("discord")?,
            conversation: ConversationId::new("channel-1")?,
            thread: None,
            reply_to: None,
        },
        sender: Participant::default(),
        body: MessageBody::text("ping")?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    };
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
    let listener = UnixListener::bind(&socket)?;
    let server = thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = listener.accept()?;
        let mut frame = String::new();
        BufReader::new(&mut stream).read_line(&mut frame)?;
        stream.write_all(
            br#"{"type":"error","recoverable":false,"message":"sk-secret-provider-detail"}
{"type":"done","status":"error"}
"#,
        )
    });
    let bridge = AgentChannelBridge::new(socket, ChannelSessionRoute::new("coder", "im")?, None);
    let inbound = InboundMessage {
        id: "message-3".to_owned(),
        target: MessageTarget {
            channel: ChannelId::new("discord")?,
            conversation: ConversationId::new("channel-1")?,
            thread: None,
            reply_to: None,
        },
        sender: Participant::default(),
        body: MessageBody::text("ping")?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    };
    let mut probe = ProgressProbe::default();
    let Err(error) = bridge.handle_with_progress(inbound, &mut probe) else {
        return Err(std::io::Error::other("agent error was not returned").into());
    };
    server
        .join()
        .map_err(|error| format!("server panicked: {error:?}"))??;
    assert!(matches!(error, super::bridge::ChannelBridgeError::Agent(_)));
    let Some(message) = probe.error else {
        return Err(std::io::Error::other("progress error was not delivered").into());
    };
    assert!(!message.contains("sk-secret-provider-detail"));
    assert!(message.contains("模型服务") || message.contains("model service"));
    Ok(())
}

#[test]
fn driver_streams_effects_before_final_delivery() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let agent_socket = root.path().join("agent.sock");
    let agent_listener = UnixListener::bind(&agent_socket)?;
    let agent = thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = agent_listener.accept()?;
        let mut request = String::new();
        BufReader::new(&mut stream).read_line(&mut request)?;
        stream.write_all(
            b"{\"type\":\"delta\",\"run\":\"run-1\",\"text\":\"pong\"}\n{\"type\":\"done\",\"run\":\"run-1\",\"status\":\"ok\"}\n",
        )
    });
    let bridge =
        AgentChannelBridge::new(agent_socket, ChannelSessionRoute::new("coder", "im")?, None);
    let channel = ChannelId::new("telegram")?;
    let config = DriverConfig {
        socket: root.path().join("channel.sock"),
        channel: channel.clone(),
        bridge,
        hub: DriverHub::default(),
    };
    let (mut runtime, mut adapter) = UnixStream::pair()?;
    let inbound = InboundMessage {
        id: "message-4".to_owned(),
        target: MessageTarget {
            channel,
            conversation: ConversationId::new("chat-1")?,
            thread: None,
            reply_to: None,
        },
        sender: Participant {
            id: "user-1".to_owned(),
            ..Participant::default()
        },
        body: MessageBody::text("ping")?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    };
    let (response, close) = driverhandle::handle(
        ChannelFrame::new(ChannelFrameBody::Inbound {
            event_id: "event-1".to_owned(),
            message: inbound,
        }),
        &config,
        &mut runtime,
    );
    let mut reader = BufReader::new(&mut adapter);
    let mut effect = String::new();
    reader.read_line(&mut effect)?;
    assert!(effect.contains("\"typing\""));
    effect.clear();
    reader.read_line(&mut effect)?;
    assert!(effect.contains("\"preview\""));
    effect.clear();
    reader.read_line(&mut effect)?;
    assert!(effect.contains("\"active\":false"));
    assert!(!close);
    assert!(matches!(
        response.map(|frame| frame.frame),
        Some(ChannelFrameBody::Deliver { .. })
    ));
    agent
        .join()
        .map_err(|_error| std::io::Error::other("agent panicked"))??;
    Ok(())
}

#[test]
fn driver_routes_non_message_event_with_structured_payload()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let agent_socket = root.path().join("agent.sock");
    let listener = UnixListener::bind(&agent_socket)?;
    let agent = thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = listener.accept()?;
        let mut request = String::new();
        BufReader::new(&mut stream).read_line(&mut request)?;
        let event_seen = serde_json::from_str::<serde_json::Value>(&request).is_ok_and(|value| {
            value
                .pointer("/payload/value/event/type")
                .and_then(serde_json::Value::as_str)
                == Some("reaction")
        });
        stream.write_all(
            b"{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"seen\"}]}\n{\"type\":\"done\",\"status\":\"ok\"}\n",
        )?;
        if !event_seen {
            return Err(std::io::Error::other(format!(
                "structured event was not forwarded: {request}"
            )));
        }
        Ok(())
    });
    let channel = ChannelId::new("telegram")?;
    let target = MessageTarget {
        channel: channel.clone(),
        conversation: ConversationId::new("chat-event")?,
        thread: None,
        reply_to: None,
    };
    let config = DriverConfig {
        socket: root.path().join("channel.sock"),
        channel,
        bridge: AgentChannelBridge::new(
            agent_socket,
            ChannelSessionRoute::new("coder", "im")?,
            None,
        ),
        hub: DriverHub::default(),
    };
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
    agent
        .join()
        .map_err(|error| std::io::Error::other(format!("agent panicked: {error:?}")))??;
    Ok(())
}

#[test]
fn driver_round_trips_runtime_command_on_the_channel_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let agent_socket = root.path().join("agent.sock");
    let agent_listener = UnixListener::bind(&agent_socket)?;
    let agent = thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = agent_listener.accept()?;
        let mut request = String::new();
        BufReader::new(&mut stream).read_line(&mut request)?;
        stream.write_all(
            br#"{"type":"approval_request","run":"run-2","id":"call-2","name":"fs.write","args":{}}
"#,
        )?;
        let mut result = String::new();
        BufReader::new(&mut stream).read_line(&mut result)?;
        if !result.contains("\"accepted\"") {
            return Err(std::io::Error::other("command result was not accepted"));
        }
        stream.write_all(
            b"{\"type\":\"delta\",\"run\":\"run-2\",\"text\":\"done\"}\n{\"type\":\"done\",\"run\":\"run-2\",\"status\":\"ok\"}\n",
        )?;
        Ok(())
    });
    let channel = ChannelId::new("telegram")?;
    let target = MessageTarget {
        channel: channel.clone(),
        conversation: ConversationId::new("chat-2")?,
        thread: None,
        reply_to: None,
    };
    let route = ChannelSessionRoute::new("coder", "im")?;
    let bridge = AgentChannelBridge::new(agent_socket, route.clone(), None);
    let config = DriverConfig {
        socket: root.path().join("channel.sock"),
        channel,
        bridge,
        hub: DriverHub::default(),
    };
    let (runtime, adapter) = UnixStream::pair()?;
    let mut adapter_writer = adapter.try_clone()?;
    let server = thread::spawn(move || super::driver::serve_once(runtime, &config));
    let inbound = InboundMessage {
        id: "event-2".to_owned(),
        target: target.clone(),
        sender: Participant {
            id: "user-2".to_owned(),
            ..Participant::default()
        },
        body: MessageBody::text("approve")?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    };
    let request_id = route.request_id_for(&inbound);
    let frame = ChannelFrame::new(ChannelFrameBody::Inbound {
        event_id: "event-2".to_owned(),
        message: inbound,
    });
    adapter_writer.write_all(&frame.encode()?)?;
    let mut reader = BufReader::new(adapter);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    assert!(line.contains("\"typing\""));
    line.clear();
    reader.read_line(&mut line)?;
    assert!(line.contains("\"command\""));
    let session = route.session_for(&target);
    let result = ChannelFrame::new(ChannelFrameBody::CommandResult {
        request_id,
        session,
        command_id: "call-2".to_owned(),
        result: ChannelCommandResult::Accepted,
    });
    adapter_writer.write_all(&result.encode()?)?;
    for expected in ["preview", "\"active\":false", "deliver"] {
        line = String::new();
        reader.read_line(&mut line)?;
        assert!(line.contains(expected), "missing {expected}: {line}");
    }
    drop(reader);
    drop(adapter_writer);
    server
        .join()
        .map_err(|_error| std::io::Error::other("driver panicked"))??;
    agent
        .join()
        .map_err(|_error| std::io::Error::other("agent panicked"))??;
    Ok(())
}

#[test]
fn driver_accepts_unsolicited_status_and_receipt_frames() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let channel = ChannelId::new("telegram")?;
    let config = DriverConfig {
        socket: root.path().join("channel.sock"),
        channel: channel.clone(),
        bridge: AgentChannelBridge::new(
            root.path().join("agent.sock"),
            ChannelSessionRoute::new("coder", "im")?,
            None,
        ),
        hub: DriverHub::default(),
    };
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
                target: MessageTarget {
                    channel: ChannelId::new("telegram")?,
                    conversation: ConversationId::new("chat-1")?,
                    thread: None,
                    reply_to: None,
                },
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
    let hub = DriverHub::default();
    let bridge = AgentChannelBridge::new(
        root.path().join("agent.sock"),
        ChannelSessionRoute::new("coder", "im")?,
        None,
    );
    let adapter_config = DriverConfig {
        socket: root.path().join("channel.sock"),
        channel: channel.clone(),
        bridge: bridge.clone(),
        hub: hub.clone(),
    };
    let control_config = DriverConfig {
        socket: root.path().join("control.sock"),
        channel: channel.clone(),
        bridge,
        hub,
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
    let mut line = String::new();
    adapter_reader.read_line(&mut line)?;
    assert!(line.contains("connected"));
    line.clear();
    adapter_reader.read_line(&mut line)?;
    assert!(line.contains("connected"));

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
    line.clear();
    control_reader.read_line(&mut line)?;
    assert!(line.contains("connected"));
    let target = MessageTarget {
        channel,
        conversation: ConversationId::new("room-1")?,
        thread: None,
        reply_to: None,
    };
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
    line.clear();
    control_reader.read_line(&mut line)?;
    assert!(line.contains("control_response"));
    assert!(line.contains("\"accepted\":true"));
    line.clear();
    adapter_reader.read_line(&mut line)?;
    assert!(line.contains("\"effect\""), "adapter frame: {line}");
    drop(control_reader);
    drop(control);
    control_thread
        .join()
        .map_err(|error| std::io::Error::other(format!("control driver panicked: {error:?}")))??;
    drop(adapter_reader);
    drop(adapter);
    adapter_thread
        .join()
        .map_err(|error| std::io::Error::other(format!("adapter driver panicked: {error:?}")))??;
    Ok(())
}
