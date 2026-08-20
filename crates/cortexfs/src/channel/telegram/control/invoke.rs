use reqwest::blocking::Client;
use serde_json::{Map, Value, json};

use super::super::{TelegramConfig, TelegramError, request};
use cortexfs_channels::MessageTarget;

pub(super) fn run(
    client: &Client,
    config: &TelegramConfig,
    target: &MessageTarget,
    name: &str,
    payload: &Value,
) -> Result<Value, TelegramError> {
    let mut fields = payload
        .as_object()
        .cloned()
        .ok_or_else(|| TelegramError::Api("payload must be an object".to_owned()))?;
    match name {
        "telegram.send_photo" => media(&mut fields, "photo", payload, "url")?,
        "telegram.send_document" => media(&mut fields, "document", payload, "url")?,
        "telegram.send_video" => media(&mut fields, "video", payload, "url")?,
        "telegram.send_audio" => media(&mut fields, "audio", payload, "url")?,
        "telegram.send_voice" => media(&mut fields, "voice", payload, "url")?,
        "telegram.send_location" => {
            require(&fields, "latitude")?;
            require(&fields, "longitude")?;
        }
        "telegram.send_poll" => {
            require(&fields, "question")?;
            require(&fields, "options")?;
        }
        "telegram.answer_callback" => {
            require(&fields, "callback_query_id")?;
            return request::call(client, config, "answerCallbackQuery", fields);
        }
        "telegram.draft_update" => {
            require(&fields, "message_id")?;
            require(&fields, "text")?;
            fields.insert("chat_id".to_owned(), json!(target.conversation.as_str()));
            return request::call(client, config, "editMessageText", fields);
        }
        _ => return Err(TelegramError::Api("unsupported operation".to_owned())),
    }
    fields.insert("chat_id".to_owned(), json!(target.conversation.as_str()));
    let method = match name {
        "telegram.send_photo" => "sendPhoto",
        "telegram.send_document" => "sendDocument",
        "telegram.send_video" => "sendVideo",
        "telegram.send_audio" => "sendAudio",
        "telegram.send_voice" => "sendVoice",
        "telegram.send_location" => "sendLocation",
        "telegram.send_poll" => "sendPoll",
        _ => return Err(TelegramError::Api("unsupported operation".to_owned())),
    };
    request::call(client, config, method, fields)
}

fn media(
    fields: &mut Map<String, Value>,
    field: &str,
    payload: &Value,
    alias: &str,
) -> Result<(), TelegramError> {
    if !fields.contains_key(field) {
        let value = payload
            .get(alias)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| TelegramError::Api(format!("{field} is missing")))?;
        fields.insert(field.to_owned(), json!(value));
    }
    Ok(())
}

fn require(fields: &Map<String, Value>, name: &str) -> Result<(), TelegramError> {
    fields
        .get(name)
        .filter(|value| !value.is_null())
        .map(|_| ())
        .ok_or_else(|| TelegramError::Api(format!("{name} is missing")))
}
