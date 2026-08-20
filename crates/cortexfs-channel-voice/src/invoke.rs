use cortexfs_channels::MessageTarget;
use reqwest::Client;
use serde_json::{Value, json};

use crate::{
    config::{ChannelKind, Config},
    error::{Error, Result},
    provider::{ActiveCall, Calls, call, control, speech},
};

#[expect(clippy::redundant_pub_crate, reason = "private driver helper")]
pub(crate) async fn run(
    config: &Config,
    client: &Client,
    calls: &mut Calls,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value> {
    if config.channel == ChannelKind::VoiceWake {
        return crate::wake::run(config, name, payload).await;
    }
    let destination = payload
        .get("destination")
        .and_then(Value::as_str)
        .or_else(|| target.map(|item| item.conversation.as_str()))
        .ok_or_else(|| error("destination is missing"))?;
    match name {
        "voice_call.start_call" | "clawdtalk.start_call" => {
            if !config.accepts(destination) {
                return Err(error("voice destination is not allowlisted"));
            }
            let id = call::place(config, client, destination).await?;
            calls.insert(format!("call:{id}"), ActiveCall { id: id.clone() });
            calls.insert(
                format!("phone:{destination}"),
                ActiveCall { id: id.clone() },
            );
            Ok(json!({"accepted":true,"call_id":id}))
        }
        "voice_call.speak" | "clawdtalk.speak" => {
            let active = active(calls, payload, destination)?;
            speech::speak(config, client, &active, text(payload)?).await?;
            Ok(json!({"accepted":true,"call_id":active.id}))
        }
        "voice_call.hangup" | "clawdtalk.hangup" => {
            let active = active(calls, payload, destination)?;
            call::hangup(config, client, &active.id).await?;
            calls.retain(|_, item| item.id != active.id);
            Ok(json!({"accepted":true,"call_id":active.id}))
        }
        "voice_call.transfer" | "clawdtalk.transfer" => {
            let to = text_field(payload, "to")?;
            if !config.accepts(to) {
                return Err(error("voice destination is not allowlisted"));
            }
            let active = active(calls, payload, destination)?;
            control::transfer(config, client, &active.id, to).await?;
            Ok(json!({"accepted":true,"call_id":active.id,"to":to}))
        }
        "voice_call.record" | "clawdtalk.record" => {
            let active = active(calls, payload, destination)?;
            control::record(config, client, &active.id).await?;
            Ok(json!({"accepted":true,"call_id":active.id}))
        }
        _ => Err(error("unsupported operation")),
    }
}

fn active(calls: &Calls, payload: &Value, destination: &str) -> Result<ActiveCall> {
    payload
        .get("call_id")
        .and_then(Value::as_str)
        .or_else(|| destination.strip_prefix("call:"))
        .and_then(|id| {
            calls
                .values()
                .find(|call| call.id == id)
                .map(|call| call.id.as_str())
        })
        .or_else(|| calls.get(destination).map(|call| call.id.as_str()))
        .map(|id| ActiveCall { id: id.to_owned() })
        .ok_or_else(|| error("active call is missing"))
}

fn text(payload: &Value) -> Result<&str> {
    payload
        .get("text")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error("text is missing"))
}

fn text_field<'a>(payload: &'a Value, name: &str) -> Result<&'a str> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error("voice destination is missing"))
}

fn error(message: &str) -> Error {
    Error::Protocol(message.to_owned())
}
