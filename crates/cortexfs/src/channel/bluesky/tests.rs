use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;

use super::{BlueskyConfig, api};

#[test]
fn api_round_trips_login_poll_send_and_seen() -> Result<(), Box<dyn std::error::Error>> {
    let (base, server) = crate::channel::http::test::server(
        "/xrpc",
        [
            r#"{"accessJwt":"access","refreshJwt":"refresh","did":"did:plc:bot"}"#,
            r#"{"notifications":[]}"#,
            "{}",
            "{}",
        ],
    )?;
    let client = Client::builder().build()?;
    let config = BlueskyConfig::new("bot.test", "app-password", base)?;
    let mut session = api::login(&client, &config)?;
    assert_eq!(session.did, "did:plc:bot");
    assert!(api::notifications(&client, &config, &mut session)?.contains("notifications"));
    api::send(
        &client,
        &config,
        &mut session,
        OutboundRequest {
            method: "POST".to_owned(),
            path: "com.atproto.repo.createRecord".to_owned(),
            content_type: "application/json".to_owned(),
            body: "{}".to_owned(),
            headers: std::collections::BTreeMap::new(),
        },
    )?;
    api::mark_seen(&client, &config, &mut session, "2026-08-17T10:00:00Z")?;
    server
        .join()
        .map_err(|error| std::io::Error::other(format!("mock server panicked: {error:?}")))?;
    Ok(())
}
