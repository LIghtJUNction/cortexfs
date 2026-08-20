use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelCodec, MessageBody, MessageTarget, OutboundMessage, platform::bluesky::BlueskyCodec,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{BlueskyConfig, api, clock};
use crate::channel::control::ChannelControlError;

mod request;
use self::request::{field, record, remove};

pub(super) fn run(
    client: &Client,
    config: &BlueskyConfig,
    session: &mut api::Session,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    match name {
        "bluesky.create_post" | "bluesky.reply" | "bluesky.quote" => {
            send_post(client, config, session, target, name, payload)
        }
        "bluesky.like" => record(
            client,
            config,
            session,
            "app.bsky.feed.like",
            &json!({"subject": subject(payload)?, "createdAt": clock::now()}),
        ),
        "bluesky.unlike" => remove(
            client,
            config,
            session,
            "app.bsky.feed.like",
            field(payload, "rkey")?,
        ),
        "bluesky.repost" => record(
            client,
            config,
            session,
            "app.bsky.feed.repost",
            &json!({"subject": subject(payload)?, "createdAt": clock::now()}),
        ),
        "bluesky.follow" => record(
            client,
            config,
            session,
            "app.bsky.graph.follow",
            &json!({"subject": field(payload, "did")?, "createdAt": clock::now()}),
        ),
        _ => Err(fail("unsupported operation")),
    }
}

fn send_post(
    client: &Client,
    config: &BlueskyConfig,
    session: &mut api::Session,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let mut target = target.cloned().ok_or_else(|| fail("target is missing"))?;
    let mut metadata = BTreeMap::from([
        ("bluesky.repo".to_owned(), session.did.clone()),
        ("bluesky.created_at".to_owned(), clock::now()),
    ]);
    if name == "bluesky.reply" {
        target.reply_to = Some(format!(
            "{}|{}",
            field(payload, "uri")?,
            field(payload, "cid")?
        ));
    }
    if name == "bluesky.quote" {
        metadata.insert(
            "bluesky.quote_uri".to_owned(),
            field(payload, "uri")?.to_owned(),
        );
        metadata.insert(
            "bluesky.quote_cid".to_owned(),
            field(payload, "cid")?.to_owned(),
        );
    }
    let message = OutboundMessage {
        target,
        body: MessageBody::text(field(payload, "text")?)
            .map_err(|error| fail(&error.to_string()))?,
        metadata,
    };
    let request = BlueskyCodec
        .encode(&message)
        .map_err(|error| fail(&error.to_string()))?;
    api::send(client, config, session, request).map_err(|error| fail(&error.to_string()))?;
    Ok(json!({"accepted":true}))
}

fn subject(payload: &Value) -> Result<Value, ChannelControlError> {
    Ok(json!({
        "uri": field(payload, "uri")?,
        "cid": field(payload, "cid")?
    }))
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
