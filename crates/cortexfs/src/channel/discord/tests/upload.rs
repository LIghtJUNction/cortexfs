use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::{Reply, config, join, receive, server, target};
use crate::channel::discord::invoke;

#[test]
fn file_retry_rebuilds_the_complete_multipart_body() -> Result<(), Box<dyn std::error::Error>> {
    let (base, requests, worker) = server([
        Reply {
            status: "503 Service Unavailable",
            headers: &[("Retry-After", "0")],
            body: r#"{"message":"retry"}"#,
        },
        Reply {
            status: "200 OK",
            headers: &[],
            body: r#"{"id":"file-1"}"#,
        },
    ])?;
    let result = invoke::run(
        &Client::new(),
        &config(format!("{base}/api/v10")),
        &target()?,
        "file-command",
        "discord.send_file",
        &json!({
            "filename":"hello.txt",
            "content_type":"text/plain",
            "data_base64":"aGVsbG8=",
            "text":"attached"
        }),
    )?;
    assert_eq!(result.get("id").and_then(Value::as_str), Some("file-1"));
    for request in [receive(&requests)?, receive(&requests)?] {
        let body = String::from_utf8_lossy(&request.body);
        assert!(
            request
                .head
                .starts_with("POST /api/v10/channels/123/messages ")
        );
        assert!(request.head.contains("multipart/form-data; boundary="));
        assert!(body.contains("payload_json"));
        assert!(body.contains("hello.txt"));
        assert!(!body.contains("file-command"));
        assert!(body.contains("hello"));
    }
    join(worker)?;
    Ok(())
}
