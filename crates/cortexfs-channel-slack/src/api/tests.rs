use std::{
    collections::BTreeMap,
    io::{Read as _, Write as _},
    net::TcpListener,
    thread,
    time::Duration,
};

use cortexfs_channels::{
    ChannelCommand, ChannelId, ConversationId, MessageBody, MessageTarget, OutboundMessage,
};
use reqwest::Client;

use crate::{
    api::{CommandOutcome, send_command, send_message},
    config::Config,
    socket::PendingKind,
};

#[tokio::test]
async fn send_message_uses_mock_slack_api_and_returns_receipt()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let worker = thread::spawn(move || -> std::io::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut request = [0_u8; 8 * 1024];
        let size = stream.read(&mut request)?;
        stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 22\r\n\r\n{\"ok\":true,\"ts\":\"1.2\"}",
        )?;
        Ok(String::from_utf8_lossy(request.get(..size).unwrap_or_default()).into_owned())
    });
    let config = Config {
        app_token: "app-token".to_owned(),
        bot_token: "bot-token".to_owned(),
        api_base: format!("http://{address}"),
        socket: "/run/cortexfs/channel/slack.sock".into(),
        reconnect_seconds: 5,
        reply_timeout: Duration::from_secs(1),
    };
    let target = MessageTarget {
        channel: ChannelId::from_static("slack"),
        conversation: ConversationId::new("C1")?,
        thread: Some("1.0".to_owned()),
        reply_to: None,
    };
    let receipt = send_message(
        &Client::new(),
        &config,
        OutboundMessage {
            target: target.clone(),
            body: MessageBody::text("hello")?,
            metadata: BTreeMap::default(),
        },
    )
    .await?;
    let request = worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    assert!(request.starts_with("POST /chat.postMessage HTTP/1.1"));
    assert!(request.contains("Bearer bot-token"));
    assert_eq!(receipt.message_id, "1.2");
    assert_eq!(receipt.target, target);
    Ok(())
}

#[tokio::test]
async fn approval_command_uses_slack_action_blocks()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let worker = thread::spawn(move || -> std::io::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut request = [0_u8; 8 * 1024];
        let size = stream.read(&mut request)?;
        stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 22\r\n\r\n{\"ok\":true,\"ts\":\"1.2\"}",
        )?;
        Ok(String::from_utf8_lossy(request.get(..size).unwrap_or_default()).into_owned())
    });
    let config = Config {
        app_token: "app-token".to_owned(),
        bot_token: "bot-token".to_owned(),
        api_base: format!("http://{address}"),
        socket: "/run/cortexfs/channel/slack.sock".into(),
        reconnect_seconds: 5,
        reply_timeout: Duration::from_secs(1),
    };
    let target = MessageTarget {
        channel: ChannelId::from_static("slack"),
        conversation: ConversationId::new("C1")?,
        thread: None,
        reply_to: None,
    };
    let outcome = send_command(
        &Client::new(),
        &config,
        &target,
        "command-1",
        &ChannelCommand::RequestApproval {
            tool: "fs.write".to_owned(),
            arguments: serde_json::json!({"path":"notes.txt"}),
        },
    )
    .await?;
    assert!(matches!(
        outcome,
        CommandOutcome::Pending(PendingKind::Approval)
    ));
    let request = worker
        .join()
        .map_err(|error| std::io::Error::other(format!("worker panicked: {error:?}")))??;
    assert!(request.contains("POST /chat.postMessage HTTP/1.1"));
    assert!(request.contains("cortexfs_command"));
    assert!(request.contains("command-1"));
    Ok(())
}
