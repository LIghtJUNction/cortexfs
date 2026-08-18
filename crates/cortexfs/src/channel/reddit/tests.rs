use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;

use super::{RedditConfig, api};

#[test]
fn api_round_trips_token_inbox_send_and_mark_read() -> Result<(), Box<dyn std::error::Error>> {
    let (base, server) = crate::channel::http::test::server(
        "",
        [
            r#"{"access_token":"access","expires_in":3600}"#,
            r#"{"data":{"children":[]}}"#,
            "{}",
            "{}",
        ],
    )?;
    let client = Client::builder().build()?;
    let config = RedditConfig {
        client_id: "client".to_owned(),
        client_secret: "secret".to_owned(),
        refresh_token: "refresh".to_owned(),
        username: "bot".to_owned(),
        subreddits: Vec::new(),
        api_base: base.clone(),
        token_url: format!("{base}/token"),
        poll_seconds: 5,
    };
    let mut session = api::login(&client, &config)?;
    assert!(api::inbox(&client, &config, &mut session)?.contains("children"));
    api::send(
        &client,
        &config,
        &mut session,
        OutboundRequest {
            method: "POST".to_owned(),
            path: "api/comment".to_owned(),
            content_type: "application/x-www-form-urlencoded".to_owned(),
            body: "thing_id=t1_parent&text=answer".to_owned(),
            headers: std::collections::BTreeMap::new(),
        },
    )?;
    api::mark_read(&client, &config, &mut session, &["t1_reply".to_owned()])?;
    server
        .join()
        .map_err(|error| std::io::Error::other(format!("mock server panicked: {error:?}")))?;
    Ok(())
}
