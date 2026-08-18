use cortexfs_channels::{ChannelCodec, platform::twitter::TwitterCodec};
use reqwest::blocking::Client;

use super::{TwitterConfig, api};

#[test]
fn api_round_trips_identity_mentions_and_send() -> Result<(), Box<dyn std::error::Error>> {
    let (base, server) = crate::channel::http::test::server(
        "",
        [
            r#"{"data":{"id":"bot"}}"#,
            r#"{"data":[],"meta":{"newest_id":"2"}}"#,
            r#"{"data":{"id":"reply"}}"#,
        ],
    )?;
    let client = Client::builder().build()?;
    let config = TwitterConfig::new("bearer", base)?;
    assert_eq!(api::me(&client, &config)?, "bot");
    assert!(api::mentions(&client, &config, "bot", Some("1"))?.contains("newest_id"));
    let id = api::send(
        &client,
        &config,
        TwitterCodec.encode(&cortexfs_channels::OutboundMessage {
            target: cortexfs_channels::MessageTarget {
                channel: cortexfs_channels::ChannelId::new("twitter")?,
                conversation: cortexfs_channels::ConversationId::new("c")?,
                thread: None,
                reply_to: Some("1".to_owned()),
            },
            body: cortexfs_channels::MessageBody::text("answer")?,
            metadata: std::collections::BTreeMap::new(),
        })?,
    )?;
    assert_eq!(id.as_deref(), Some("reply"));
    server
        .join()
        .map_err(|error| std::io::Error::other(format!("mock server panicked: {error:?}")))?;
    Ok(())
}

#[test]
fn debug_redacts_bearer_token() -> Result<(), Box<dyn std::error::Error>> {
    let config = TwitterConfig::new("secret", "https://api.x.com/2")?;
    let debug = format!("{config:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("secret"));
    Ok(())
}

#[test]
fn chunks_preserve_unicode_and_boundaries() {
    let text = format!("{}{}tail", "a".repeat(279), "😀");
    let chunks = super::text::chunks(&text, 280);
    assert_eq!(chunks.concat(), text);
    assert!(chunks.iter().all(|chunk| chunk.len() <= 280));
}

#[test]
fn empty_allowlist_denies_and_wildcard_allows() -> Result<(), Box<dyn std::error::Error>> {
    let config = TwitterConfig::new("token", "https://api.x.com/2")?;
    assert!(!config.accepts("1", Some("alice")));
    let config = config.with_allowed_users(vec!["*".to_owned()]);
    assert!(config.accepts("1", Some("alice")));
    Ok(())
}
