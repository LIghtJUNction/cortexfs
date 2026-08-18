use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::thread;
use std::time::{Duration, Instant};

use cortexfs::channel::{bridge::AgentChannelBridge, http::HttpRequest};
use cortexfs_channels::{ChannelId, ChannelSessionRoute, platform::discord::DiscordCodec};

use super::{Platform, WebhookConfig, handle};

#[test]
fn webhook_routes_provider_event_through_the_common_bridge()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let agent_socket = root.path().join("agent.sock");
    let agent_listener = UnixListener::bind(&agent_socket)?;
    let agent = thread::spawn(move || -> Result<(), std::io::Error> {
        let (mut stream, _) = agent_listener.accept()?;
        let mut request = String::new();
        BufReader::new(&mut stream).read_line(&mut request)?;
        let event_seen = serde_json::from_str::<serde_json::Value>(&request).is_ok_and(|value| {
            value
                .pointer("/payload/value/event/type")
                .and_then(serde_json::Value::as_str)
                == Some("reaction")
        });
        let channel_seen = serde_json::from_str::<serde_json::Value>(&request).is_ok_and(|value| {
            value
                .pointer("/payload/value/origin/endpoint")
                .and_then(serde_json::Value::as_str)
                == Some("discord.primary")
        });
        if !event_seen || !channel_seen {
            return Err(std::io::Error::other(format!(
                "provider event was not forwarded: {request}"
            )));
        }
        stream.write_all(
            b"{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"seen\"}]}\n{\"type\":\"done\",\"status\":\"ok\"}\n",
        )
    });
    let outbound_listener = TcpListener::bind(("127.0.0.1", 0))?;
    let outbound_addr = outbound_listener.local_addr()?;
    outbound_listener.set_nonblocking(true)?;
    let outbound = thread::spawn(move || -> Result<(), std::io::Error> {
        let deadline = Instant::now() + Duration::from_secs(2);
        for _ in 0..2 {
            let (mut stream, _) = loop {
                match outbound_listener.accept() {
                    Ok(pair) => break pair,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => return Err(error),
                }
            };
            let mut request = [0_u8; 4096];
            let _bytes = stream.read(&mut request)?;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        }
        Ok(())
    });
    let config = WebhookConfig {
        bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        path: "/webhook".to_owned(),
        platform: Platform::Discord,
        outbound_url: format!("http://{outbound_addr}/webhook"),
        token: None,
        verify_token: None,
        channel: Some(ChannelId::new("discord.primary")?),
    };
    let bridge =
        AgentChannelBridge::new(agent_socket, ChannelSessionRoute::new("coder", "im")?, None);
    let client = reqwest::blocking::Client::new();
    let response = handle(
        &config,
        &client,
        &DiscordCodec,
        &bridge,
        &HttpRequest {
            method: "POST".to_owned(),
            path: "/webhook".to_owned(),
            headers: std::collections::BTreeMap::new(),
            body: r#"{"t":"MESSAGE_REACTION_ADD","d":{"channel_id":"c","message_id":"m","user_id":"u","emoji":{"name":"👍"}}}"#.to_owned(),
        },
    );
    let agent_result = agent
        .join()
        .map_err(|error| std::io::Error::other(format!("agent panicked: {error:?}")))
        .and_then(|result| result);
    let outbound_result = outbound
        .join()
        .map_err(|error| std::io::Error::other(format!("outbound panicked: {error:?}")))
        .and_then(|result| result);
    assert!(agent_result.is_ok(), "agent result: {agent_result:?}");
    assert!(
        outbound_result.is_ok(),
        "outbound result: {outbound_result:?}"
    );
    assert_eq!(response.status, 200);
    Ok(())
}
