#![expect(
    clippy::redundant_pub_crate,
    reason = "provider calls are private driver plumbing"
)]

use reqwest::{Client, RequestBuilder, Response};
use serde_json::{Value, json};

use crate::{
    config::{Config, Provider},
    error::{Error, Result},
};
pub(crate) async fn place(config: &Config, client: &Client, to: &str) -> Result<String> {
    let callback = config
        .webhook_base
        .as_deref()
        .map(|base| format!("{}/voice/status", base.trim_end_matches('/')));
    let response = match config.provider {
        Provider::Twilio => {
            let mut form = vec![("To", to.to_owned()), ("From", config.from_number.clone())];
            if let Some(callback) = callback {
                form.push(("StatusCallback", callback));
            }
            auth(config, client.post(format!("{}/Accounts/{}/Calls.json", config.api_base, config.account_id)))
                .form(&form)
                .send().await?
        }
        Provider::Telnyx => auth(config, client.post(format!("{}/calls", config.api_base)))
            .json(&json!({"connection_id": config.account_id, "to": to, "from": config.from_number, "webhook_url": callback}))
            .send().await?,
        Provider::Plivo => auth(config, client.post(format!("{}/Account/{}/Call/", config.api_base, config.account_id)))
            .json(&json!({"to": to, "from": config.from_number, "answer_url": callback}))
            .send().await?,
    };
    let value = json_response(response).await?;
    call_id(config.provider, &value)
}

pub(crate) async fn hangup(config: &Config, client: &Client, id: &str) -> Result<()> {
    let response = match config.provider {
        Provider::Twilio => {
            auth(
                config,
                client.post(format!(
                    "{}/Accounts/{}/Calls/{}.json",
                    config.api_base, config.account_id, id
                )),
            )
            .form(&[("Status", "completed")])
            .send()
            .await?
        }
        Provider::Telnyx => {
            auth(
                config,
                client.post(format!("{}/calls/{}/actions/hangup", config.api_base, id)),
            )
            .json(&json!({}))
            .send()
            .await?
        }
        Provider::Plivo => {
            auth(
                config,
                client.post(format!(
                    "{}/Account/{}/Call/{}/",
                    config.api_base, config.account_id, id
                )),
            )
            .json(&json!({"legs": "aleg"}))
            .send()
            .await?
        }
    };
    let _value = json_response(response).await?;
    Ok(())
}

pub(crate) fn auth(config: &Config, client: RequestBuilder) -> RequestBuilder {
    match config.provider {
        Provider::Telnyx => client.bearer_auth(&config.auth_token),
        Provider::Twilio | Provider::Plivo => {
            client.basic_auth(&config.account_id, Some(&config.auth_token))
        }
    }
}

pub(crate) async fn json_response(response: Response) -> Result<Value> {
    let status = response.status();
    if !status.is_success() {
        return Err(Error::Protocol(format!(
            "voice provider rejected request: HTTP {status}"
        )));
    }
    let body = response.text().await?;
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&body)?)
}

fn call_id(provider: Provider, value: &Value) -> Result<String> {
    let paths: &[&[&str]] = match provider {
        Provider::Twilio => &[&["sid"], &["call_sid"]],
        Provider::Telnyx => &[&["data", "call_control_id"], &["call_control_id"]],
        Provider::Plivo => &[&["request_uuid"], &["call_uuid"]],
    };
    paths
        .iter()
        .find_map(|path| path_value(value, path))
        .map(str::to_owned)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::Protocol("voice provider response has no call id".to_owned()))
}

fn path_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |value, key| value.get(*key))
        .and_then(Value::as_str)
}
