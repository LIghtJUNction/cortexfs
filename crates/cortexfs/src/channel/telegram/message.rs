use reqwest::blocking::Client;
use serde_json::{Map, Value, json};

use super::{TelegramConfig, TelegramError, request};

const MAX_TEXT_CHARS: usize = 4_000;

pub(super) fn react(
    client: &Client,
    config: &TelegramConfig,
    chat: &str,
    message: &str,
    emoji: Option<&str>,
) -> Result<(), TelegramError> {
    let mut fields = Map::new();
    fields.insert("chat_id".to_owned(), json!(chat));
    fields.insert("message_id".to_owned(), json!(message));
    fields.insert(
        "reaction".to_owned(),
        emoji.map_or_else(
            || json!([]),
            |emoji| json!([{"type": "emoji", "emoji": emoji}]),
        ),
    );
    let _ignored = request::call(client, config, "setMessageReaction", fields)?;
    Ok(())
}

pub(super) fn typing(
    client: &Client,
    config: &TelegramConfig,
    chat: &str,
) -> Result<(), TelegramError> {
    let mut fields = Map::new();
    fields.insert("chat_id".to_owned(), json!(chat));
    fields.insert("action".to_owned(), json!("typing"));
    let _ignored = request::call(client, config, "sendChatAction", fields)?;
    Ok(())
}

pub(super) fn create(
    client: &Client,
    config: &TelegramConfig,
    chat: &str,
    source: &str,
    thread: Option<&str>,
) -> Result<String, TelegramError> {
    let mut fields = Map::new();
    fields.insert("chat_id".to_owned(), json!(chat));
    fields.insert("text".to_owned(), json!("⏳ 思考中…"));
    fields.insert("reply_to_message_id".to_owned(), json!(source));
    if let Some(thread) = thread {
        fields.insert("message_thread_id".to_owned(), json!(thread));
    }
    let value = request::call(client, config, "sendMessage", fields)?;
    value
        .pointer("/result/message_id")
        .map(Value::to_string)
        .ok_or_else(|| TelegramError::Api("Telegram progress message has no id".to_owned()))
}

pub(super) fn edit(
    client: &Client,
    config: &TelegramConfig,
    chat: &str,
    message: &str,
    text: &str,
) -> Result<(), TelegramError> {
    let mut fields = Map::new();
    fields.insert("chat_id".to_owned(), json!(chat));
    fields.insert("message_id".to_owned(), json!(message));
    fields.insert("text".to_owned(), json!(text));
    let _ignored = request::call(client, config, "editMessageText", fields)?;
    Ok(())
}

pub(super) fn send_text(
    client: &Client,
    config: &TelegramConfig,
    chat: &str,
    text: &str,
) -> Result<(), TelegramError> {
    let mut chunk = String::new();
    let mut count = 0_usize;
    for character in text.chars() {
        chunk.push(character);
        count += 1;
        if count == MAX_TEXT_CHARS {
            send_chunk(client, config, chat, &chunk)?;
            chunk.clear();
            count = 0;
        }
    }
    if !chunk.is_empty() {
        send_chunk(client, config, chat, &chunk)?;
    }
    Ok(())
}

fn send_chunk(
    client: &Client,
    config: &TelegramConfig,
    chat: &str,
    text: &str,
) -> Result<(), TelegramError> {
    let mut fields = Map::new();
    fields.insert("chat_id".to_owned(), json!(chat));
    fields.insert("text".to_owned(), json!(text));
    let _ignored = request::call(client, config, "sendMessage", fields)?;
    Ok(())
}
