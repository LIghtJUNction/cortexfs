use base64::Engine as _;
use serde_json::Value;
use sha2::Digest;

use super::super::DingTalkConfig;
use crate::channel::control::ChannelControlError;

pub(super) fn run(config: &DingTalkConfig, payload: &Value) -> Result<String, ChannelControlError> {
    let timestamp = value(payload, "timestamp")?;
    let body = value(payload, "body")?;
    let mut key = config.client_secret.as_bytes().to_vec();
    if key.len() > 64 {
        key = sha2::Sha256::digest(&key).to_vec();
    }
    key.resize(64, 0);
    let mut inner = [0x36_u8; 64];
    let mut outer = [0x5c_u8; 64];
    for (inner_slot, (outer_slot, byte)) in inner.iter_mut().zip(outer.iter_mut().zip(key.iter())) {
        *inner_slot ^= byte;
        *outer_slot ^= byte;
    }
    let mut message = format!("{timestamp}\n").into_bytes();
    message.extend_from_slice(body.as_bytes());
    let mut first = sha2::Sha256::new();
    first.update(inner);
    first.update(message);
    let mut second = sha2::Sha256::new();
    second.update(outer);
    second.update(first.finalize());
    Ok(base64::engine::general_purpose::STANDARD.encode(second.finalize()))
}

fn value<'a>(payload: &'a Value, name: &'static str) -> Result<&'a str, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| fail(&format!("{name} is missing")))
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
