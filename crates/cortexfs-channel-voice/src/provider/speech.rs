#![expect(
    clippy::redundant_pub_crate,
    reason = "provider speech actions are private driver plumbing"
)]

use cortexfs_channels::OutboundMessage;
use reqwest::Client;
use serde_json::json;

use crate::{
    config::{Config, Provider},
    error::Result,
};

use super::{ActiveCall, Calls, call};

pub(crate) async fn apply(
    config: &Config,
    client: &Client,
    calls: &mut Calls,
    active: &ActiveCall,
    message: &OutboundMessage,
) -> Result<String> {
    if message.metadata.get("voice_action").map(String::as_str) == Some("hangup") {
        call::hangup(config, client, &active.id).await?;
        remove(calls, &active.id);
    } else if !message.body.text.is_empty() {
        speak(config, client, active, &message.body.text).await?;
        if let Some(delay) = config.hangup_after {
            tokio::time::sleep(delay).await;
            call::hangup(config, client, &active.id).await?;
            remove(calls, &active.id);
        }
    }
    Ok(active.id.clone())
}

pub(crate) async fn speak(
    config: &Config,
    client: &Client,
    active: &ActiveCall,
    text: &str,
) -> Result<()> {
    let response = match config.provider {
        Provider::Twilio => call::auth(config, client.post(format!("{}/Accounts/{}/Calls/{}.json", config.api_base, config.account_id, active.id)))
            .form(&[("Twiml", format!("<Response><Say>{}</Say></Response>", xml_escape(text)))])
            .send().await?,
        Provider::Telnyx => call::auth(config, client.post(format!("{}/calls/{}/actions/speak", config.api_base, active.id)))
            .json(&json!({"payload": text, "payload_type": "text", "voice": "female", "language": "en-US"}))
            .send().await?,
        Provider::Plivo => call::auth(config, client.post(format!("{}/Account/{}/Call/{}/Speak/", config.api_base, config.account_id, active.id)))
            .json(&json!({"text": text, "voice": "WOMAN", "language": "en-US"}))
            .send().await?,
    };
    let _value = call::json_response(response).await?;
    Ok(())
}

fn remove(calls: &mut Calls, id: &str) {
    calls.retain(|_, call| call.id != id);
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
