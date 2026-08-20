use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::{Reply, config, join, receive, server, target};
use crate::channel::discord::invoke;

#[test]
fn embed_retries_rate_limit_with_stable_nonce() -> Result<(), Box<dyn std::error::Error>> {
    let (base, requests, worker) = server([
        Reply {
            status: "429 Too Many Requests",
            headers: &[("Retry-After", "0")],
            body: r#"{"retry_after":0}"#,
        },
        Reply {
            status: "200 OK",
            headers: &[],
            body: r#"{"id":"remote-1"}"#,
        },
    ])?;
    let result = invoke::run(
        &Client::new(),
        &config(format!("{base}/api/v10")),
        &target()?,
        "command-1",
        "discord.send_embed",
        &json!({"title":"hello","description":"world"}),
    )?;
    assert_eq!(result.get("id").and_then(Value::as_str), Some("remote-1"));
    let first = receive(&requests)?;
    let second = receive(&requests)?;
    assert!(
        first
            .head
            .starts_with("POST /api/v10/channels/123/messages ")
    );
    assert!(first.head.contains("authorization: Bot secret-token\r\n"));
    let first: Value = serde_json::from_slice(&first.body)?;
    let second: Value = serde_json::from_slice(&second.body)?;
    assert_eq!(first, second);
    assert_eq!(
        first.get("enforce_nonce").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        first.get("nonce").and_then(Value::as_str).map(str::len),
        Some(24)
    );
    assert_eq!(first.pointer("/allowed_mentions/parse"), Some(&json!([])));
    join(worker)?;
    Ok(())
}
