use cortexfs_channels::{ChannelCodec, ChannelId, ConversationId, MessageBody, MessageTarget};
use reqwest::blocking::Client;

use super::{MochatConfig, api};

#[test]
fn api_round_trips_receive_and_send() -> Result<(), Box<dyn std::error::Error>> {
    let (base, server) = crate::channel::http::test::server(
        "",
        [
            r#"{"data":[{"messageId":"m1","fromUserId":"u1","content":{"text":"hello"}}]}"#,
            r#"{"code":0}"#,
        ],
    )?;
    let client = Client::builder().build()?;
    let config = MochatConfig::new(base, "token")?;
    assert!(api::receive(&client, &config, Some("m0"))?.contains("m1"));
    let message = cortexfs_channels::OutboundMessage {
        target: MessageTarget {
            channel: ChannelId::new("mochat")?,
            conversation: ConversationId::new("u1")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("answer")?,
        metadata: std::collections::BTreeMap::new(),
    };
    api::send(
        &client,
        &config,
        cortexfs_channels::platform::mochat::MochatCodec.encode(&message)?,
    )?;
    server
        .join()
        .map_err(|error| std::io::Error::other(format!("mock server panicked: {error:?}")))?;
    Ok(())
}

#[test]
fn debug_redacts_token() -> Result<(), Box<dyn std::error::Error>> {
    let config = MochatConfig::new("https://mochat.example", "secret")?;
    let debug = format!("{config:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("secret"));
    Ok(())
}
