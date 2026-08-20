#![expect(
    clippy::redundant_pub_crate,
    reason = "provider controls are private driver plumbing"
)]

use reqwest::Client;
use serde_json::json;

use crate::{
    config::{Config, Provider},
    error::Result,
};

use super::call::{auth, json_response};

pub(crate) async fn transfer(config: &Config, client: &Client, id: &str, to: &str) -> Result<()> {
    let response = match config.provider {
        Provider::Twilio => {
            auth(
                config,
                client.post(format!(
                    "{}/Accounts/{}/Calls/{}.json",
                    config.api_base, config.account_id, id
                )),
            )
            .form(&[(
                "Twiml",
                format!("<Response><Dial>{}</Dial></Response>", xml_escape(to)),
            )])
            .send()
            .await?
        }
        Provider::Telnyx => {
            auth(
                config,
                client.post(format!("{}/calls/{id}/actions/transfer", config.api_base)),
            )
            .json(&json!({"to":to}))
            .send()
            .await?
        }
        Provider::Plivo => {
            auth(
                config,
                client.post(format!(
                    "{}/Account/{}/Call/{id}/",
                    config.api_base, config.account_id
                )),
            )
            .json(&json!({"legs":"aleg","aleg_url":to}))
            .send()
            .await?
        }
    };
    let _ignored = json_response(response).await?;
    Ok(())
}

pub(crate) async fn record(config: &Config, client: &Client, id: &str) -> Result<()> {
    let response = match config.provider {
        Provider::Twilio => {
            auth(
                config,
                client.post(format!(
                    "{}/Accounts/{}/Calls/{id}.json",
                    config.api_base, config.account_id
                )),
            )
            .form(&[("Record", "true")])
            .send()
            .await?
        }
        Provider::Telnyx => {
            auth(
                config,
                client.post(format!(
                    "{}/calls/{id}/actions/record_start",
                    config.api_base
                )),
            )
            .json(&json!({}))
            .send()
            .await?
        }
        Provider::Plivo => {
            auth(
                config,
                client.post(format!(
                    "{}/Account/{}/Call/{id}/Record/",
                    config.api_base, config.account_id
                )),
            )
            .json(&json!({}))
            .send()
            .await?
        }
    };
    let _ignored = json_response(response).await?;
    Ok(())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
