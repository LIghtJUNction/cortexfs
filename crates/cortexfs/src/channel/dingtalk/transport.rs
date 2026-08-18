use serde_json::{Value, json};
use url::Url;

use super::DingTalkError;

pub(super) fn websocket_url(endpoint: &str, ticket: &str) -> Result<String, DingTalkError> {
    let mut url = Url::parse(endpoint).map_err(|error| DingTalkError::Config(error.to_string()))?;
    url.query_pairs_mut().append_pair("ticket", ticket);
    Ok(url.to_string())
}

pub(super) fn ack(root: &Value) -> String {
    let message_id = root
        .pointer("/headers/messageId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    json!({"code":200,"headers":{"contentType":"application/json","messageId":message_id},"message":"OK","data":""}).to_string()
}
