use reqwest::blocking::Client;
use serde_json::json;

use super::{Reply, config, join, server, target};
use crate::channel::discord::{DiscordError, invoke};

#[test]
fn authentication_and_config_errors_are_redacted() -> Result<(), Box<dyn std::error::Error>> {
    let (base, _requests, worker) = server([Reply {
        status: "401 Unauthorized",
        headers: &[],
        body: r#"{"message":"secret-token internal response"}"#,
    }])?;
    let config = config(format!("{base}/api/v10"));
    let error = match invoke::run(
        &Client::new(),
        &config,
        &target()?,
        "command",
        "discord.send_embed",
        &json!({"title":"hello"}),
    ) {
        Err(error) => error,
        Ok(_value) => return Err(std::io::Error::other("401 must fail").into()),
    };
    assert!(matches!(error, DiscordError::Authentication));
    assert!(!error.to_string().contains("secret-token"));
    assert!(!format!("{config:?}").contains("secret-token"));
    join(worker)?;
    Ok(())
}

#[test]
fn unsafe_file_and_conversation_inputs_fail_before_http() -> Result<(), Box<dyn std::error::Error>>
{
    let mut target = target()?;
    target.conversation = cortexfs_channels::ConversationId::new("not-a-snowflake")?;
    assert!(matches!(
        invoke::run(
            &Client::new(),
            &config("http://127.0.0.1:9".to_owned()),
            &target,
            "command",
            "discord.send_file",
            &json!({"filename":"../secret","data_base64":"eA=="}),
        ),
        Err(DiscordError::Invalid("conversation"))
    ));
    Ok(())
}
