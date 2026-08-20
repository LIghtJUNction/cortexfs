use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::{Reply, config, join, receive, server, target};
use crate::channel::discord::invoke;

#[test]
fn command_operations_use_discord_api_routes() -> Result<(), Box<dyn std::error::Error>> {
    let (base, requests, worker) = server([
        Reply {
            status: "200 OK",
            headers: &[],
            body: r#"{"id":"registered"}"#,
        },
        Reply {
            status: "204 No Content",
            headers: &[],
            body: "",
        },
        Reply {
            status: "200 OK",
            headers: &[],
            body: r#"{"id":"gate-1"}"#,
        },
        Reply {
            status: "200 OK",
            headers: &[],
            body: r#"{"id":"gate-1"}"#,
        },
        Reply {
            status: "200 OK",
            headers: &[],
            body: r#"{"id":"draft-1"}"#,
        },
    ])?;
    let client = Client::new();
    let config = config(format!("{base}/api/v10"));
    let target = target()?;
    let registered = invoke::run(
        &client,
        &config,
        &target,
        "register",
        "discord.register_command",
        &json!({"command":{"name":"status","description":"show status"}}),
    )?;
    assert_eq!(
        registered.get("id").and_then(Value::as_str),
        Some("registered")
    );
    invoke::run(
        &client,
        &config,
        &target,
        "autocomplete",
        "discord.autocomplete",
        &json!({"interaction_id":"i1","interaction_token":"t1","choices":[{"name":"one","value":"1"}]}),
    )?;
    let gate = invoke::run(
        &client,
        &config,
        &target,
        "gate",
        "discord.gate_prompt",
        &json!({"title":"Approve","description":"Continue?","choices":[{"id":"yes","label":"Yes"}]}),
    )?;
    assert_eq!(gate.get("id").and_then(Value::as_str), Some("gate-1"));
    invoke::run(
        &client,
        &config,
        &target,
        "finalize",
        "discord.gate_finalize",
        &json!({"message_id":"gate-1","outcome":"Approved"}),
    )?;
    invoke::run(
        &client,
        &config,
        &target,
        "draft",
        "discord.draft_update",
        &json!({"message_id":"draft-1","text":"working"}),
    )?;
    let register = receive(&requests)?;
    assert!(
        register
            .head
            .contains("POST /api/v10/applications/app/commands")
    );
    let autocomplete = receive(&requests)?;
    assert!(
        autocomplete
            .head
            .contains("POST /api/v10/interactions/i1/t1/callback")
    );
    let gate = receive(&requests)?;
    assert!(gate.head.contains("POST /api/v10/channels/123/messages"));
    assert!(String::from_utf8_lossy(&gate.body).contains("custom_id"));
    let finalize = receive(&requests)?;
    assert!(
        finalize
            .head
            .contains("PATCH /api/v10/channels/123/messages/gate-1")
    );
    assert!(String::from_utf8_lossy(&finalize.body).contains("components"));
    let draft = receive(&requests)?;
    assert!(
        draft
            .head
            .contains("PATCH /api/v10/channels/123/messages/draft-1")
    );
    join(worker)?;
    Ok(())
}
