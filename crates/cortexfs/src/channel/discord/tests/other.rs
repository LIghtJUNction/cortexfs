use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::{Reply, config, join, receive, server, target};
use crate::channel::discord::invoke;

#[test]
fn thread_recovers_existing_resource_and_component_posts() -> Result<(), Box<dyn std::error::Error>>
{
    let (base, requests, worker) = server([
        Reply {
            status: "400 Bad Request",
            headers: &[],
            body: r#"{"message":"thread exists"}"#,
        },
        Reply {
            status: "200 OK",
            headers: &[],
            body: r#"{"id":"456","parent_id":"123","type":11}"#,
        },
        Reply {
            status: "200 OK",
            headers: &[],
            body: r#"{"id":"component-1"}"#,
        },
    ])?;
    let config = config(format!("{base}/api/v10"));
    let client = Client::new();
    let thread = invoke::run(
        &client,
        &config,
        &target()?,
        "thread-command",
        "discord.create_thread",
        &json!({"message_id":"456","name":"discussion"}),
    )?;
    assert_eq!(thread.get("id").and_then(Value::as_str), Some("456"));
    let component = invoke::run(
        &client,
        &config,
        &target()?,
        "component-command",
        "discord.send_component",
        &json!({"components":[{"type":1,"components":[]}]}),
    )?;
    assert_eq!(
        component.get("id").and_then(Value::as_str),
        Some("component-1")
    );
    let create = receive(&requests)?;
    let lookup = receive(&requests)?;
    let post = receive(&requests)?;
    assert!(
        create
            .head
            .starts_with("POST /api/v10/channels/123/messages/456/threads ")
    );
    assert!(lookup.head.starts_with("GET /api/v10/channels/456 "));
    assert!(
        post.head
            .starts_with("POST /api/v10/channels/123/messages ")
    );
    join(worker)?;
    Ok(())
}
