use std::collections::BTreeMap;

use serde_json::Value;

use super::super::super::ChannelError;

pub(super) struct Mail {
    pub id: Option<String>,
    pub from: Option<String>,
    pub subject: Option<String>,
    pub thread: Option<String>,
    pub reply_to: Option<String>,
    pub body: String,
    pub timestamp_ms: Option<u64>,
    pub metadata: BTreeMap<String, String>,
}

pub(super) fn parse(payload: &str) -> Result<Option<Mail>, ChannelError> {
    if payload.trim_start().starts_with('{') {
        return json(payload).map(Some);
    }
    rfc822_parse(payload).map(Some)
}

fn json(payload: &str) -> Result<Mail, ChannelError> {
    let root: Value = serde_json::from_str(payload)
        .map_err(|error| ChannelError::Protocol(format!("invalid email JSON: {error}")))?;
    let body = root
        .get("text")
        .or_else(|| root.get("body"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if body.is_empty() {
        return Err(ChannelError::InvalidMessage(
            "email body is empty".to_owned(),
        ));
    }
    Ok(Mail {
        id: text(&root, "id").or_else(|| text(&root, "message_id")),
        from: text(&root, "from"),
        subject: text(&root, "subject"),
        thread: text(&root, "thread_id").or_else(|| text(&root, "threadId")),
        reply_to: text(&root, "in_reply_to").or_else(|| text(&root, "inReplyTo")),
        body: body.to_owned(),
        timestamp_ms: root.get("timestamp_ms").and_then(Value::as_u64),
        metadata: BTreeMap::new(),
    })
}

fn rfc822_parse(payload: &str) -> Result<Mail, ChannelError> {
    let (header_text, body) = payload
        .split_once("\r\n\r\n")
        .or_else(|| payload.split_once("\n\n"))
        .ok_or_else(|| ChannelError::Protocol("email headers are missing".to_owned()))?;
    let headers = headers(header_text);
    if body.is_empty() {
        return Err(ChannelError::InvalidMessage(
            "email body is empty".to_owned(),
        ));
    }
    let from = headers.get("from").cloned();
    let subject = headers.get("subject").cloned();
    let thread = headers
        .get("references")
        .and_then(|value| value.split_whitespace().last())
        .map(str::to_owned)
        .or_else(|| headers.get("in-reply-to").cloned());
    Ok(Mail {
        id: headers.get("message-id").cloned(),
        from,
        subject,
        thread,
        reply_to: headers.get("in-reply-to").cloned(),
        body: body.trim().to_owned(),
        timestamp_ms: None,
        metadata: BTreeMap::new(),
    })
}

fn headers(input: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut current = String::new();
    for line in input.lines() {
        if line.starts_with(char::is_whitespace) {
            current.push_str(line.trim());
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            current = name.to_ascii_lowercase();
            result.insert(current.clone(), value.trim().to_owned());
        }
    }
    result
}

fn text(root: &Value, name: &str) -> Option<String> {
    root.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
