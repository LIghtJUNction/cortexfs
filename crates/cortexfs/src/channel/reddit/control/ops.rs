use cortexfs_channels::MessageTarget;
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{RedditConfig, api};
use crate::channel::control::ChannelControlError;

use super::request::{post, target_id, value};

pub(super) fn run(
    client: &Client,
    config: &RedditConfig,
    session: &mut api::Session,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let result = match name {
        "reddit.submit_post" => post(
            client,
            config,
            session,
            "api/submit",
            [
                ("sr", value(payload, "subreddit")?),
                ("kind", "self".to_owned()),
                ("title", value(payload, "title")?),
                ("text", value(payload, "text")?),
            ],
        ),
        "reddit.comment" | "reddit.reply" => post(
            client,
            config,
            session,
            "api/comment",
            [
                ("thing_id", target_id(target, payload, "thing_id")?),
                ("text", value(payload, "text")?),
            ],
        ),
        "reddit.edit" => post(
            client,
            config,
            session,
            "api/editusertext",
            [
                ("thing_id", target_id(target, payload, "thing_id")?),
                ("text", value(payload, "text")?),
            ],
        ),
        "reddit.delete" => post(
            client,
            config,
            session,
            "api/del",
            [("id", target_id(target, payload, "id")?)],
        ),
        "reddit.vote" => post(
            client,
            config,
            session,
            "api/vote",
            [
                ("id", target_id(target, payload, "id")?),
                ("dir", value(payload, "dir")?),
            ],
        ),
        "reddit.flair" => post(
            client,
            config,
            session,
            "api/selectflair",
            [
                ("link", target_id(target, payload, "link")?),
                ("flair_template_id", value(payload, "flair_template_id")?),
            ],
        ),
        _ => Err(fail("unsupported operation")),
    }?;
    Ok(json!({"accepted": result}))
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
