use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::thread;

use cortexfs_channels::{
    ChannelId, ChannelSessionRoute, ConversationId, InboundMessage, MessageBody, MessageTarget,
    Participant,
};

use super::bridge::AgentChannelBridge;
use super::bridge::ChannelProgressSink;

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
        if !frame.contains("\"op\":\"send\"") || !frame.contains("\"scope\":\"private\"") {
            return Err(std::io::Error::other("invalid agent session frame"));
        }
        stream.write_all(
            b"{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"pong\"}]}\n{\"type\":\"done\",\"status\":\"ok\"}\n",
        )
    });
    let bridge = AgentChannelBridge::new(socket, ChannelSessionRoute::new("coder", "im")?, None);
    let reply = bridge.handle(InboundMessage {
        id: "message-1".to_owned(),
        target: MessageTarget {
            channel: ChannelId::new("telegram")?,
            conversation: ConversationId::new("chat-1")?,
            thread: None,
            reply_to: None,
        },
        sender: Participant::default(),
        body: MessageBody::text("ping")?,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    })?;
    server
        .join()
        .map_err(|error| format!("server panicked: {error:?}"))??;
    assert_eq!(reply.body.text, "pong");
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
    assert!(message.contains("model service"));
    Ok(())
}
