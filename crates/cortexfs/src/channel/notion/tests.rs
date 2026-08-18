use cortexfs_channels::{ChannelCodec, MessageBody, MessageTarget, OutboundMessage};
use reqwest::blocking::Client;

use super::{NotionConfig, api};

#[test]
fn debug_redacts_token() -> Result<(), Box<dyn std::error::Error>> {
    let config = NotionConfig::new("https://notion.example/v1", "secret", "database")?;
    let debug = format!("{config:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("secret"));
    Ok(())
}

#[test]
fn api_updates_status_and_outbound_page() -> Result<(), Box<dyn std::error::Error>> {
    let (base, server) = crate::channel::http::test::server(
        "",
        [
            r#"{"properties":{"Status":{"type":"select"}}}"#,
            r#"{"results":[]}"#,
            "{}",
            "{}",
        ],
    )?;
    let config = NotionConfig::new(base, "secret", "database")?;
    let client = Client::builder().build()?;
    assert_eq!(api::status_type(&client, &config)?, "select");
    assert!(api::pending(&client, &config, "select")?.is_empty());
    let codec = cortexfs_channels::platform::notion::NotionCodec::default();
    api::mark_running(&client, &config, &codec, "page-1")?;
    let request = codec.encode(&OutboundMessage {
        target: MessageTarget {
            channel: cortexfs_channels::ChannelId::new("notion")?,
            conversation: cortexfs_channels::ConversationId::new("page-1")?,
            thread: None,
            reply_to: None,
        },
        body: MessageBody::text("done")?,
        metadata: std::collections::BTreeMap::new(),
    })?;
    api::send_outbound(&client, &config, &request)?;
    server
        .join()
        .map_err(|error| std::io::Error::other(format!("mock server panicked: {error:?}")))?;
    Ok(())
}
