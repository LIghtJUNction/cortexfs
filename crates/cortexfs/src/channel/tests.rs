use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::thread;

use cortexfs_channels::{
    ChannelId, ChannelSessionRoute, ConversationId, InboundMessage, MessageBody, MessageTarget,
    Participant,
};

use super::bridge::AgentChannelBridge;

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
