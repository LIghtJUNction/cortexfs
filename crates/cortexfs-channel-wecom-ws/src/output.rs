#![expect(
    clippy::redundant_pub_crate,
    reason = "output helpers are private relay plumbing"
)]

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

const MAX_FRAME_BYTES: usize = 8_000;
static NEXT_STREAM: AtomicU64 = AtomicU64::new(1);

pub(crate) fn reply_frames(req_id: &str, text: &str) -> Vec<String> {
    let stream_id = format!("cortexfs-{:x}", NEXT_STREAM.fetch_add(1, Ordering::Relaxed));
    let chunks = chunks(text);
    let total = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, content)| {
            json!({
                "cmd": "aibot_respond_msg",
                "headers": {"req_id": req_id},
                "body": {
                    "msgtype": "stream",
                    "stream": {
                        "id": stream_id,
                        "finish": index + 1 == total,
                        "content": content,
                    }
                }
            })
            .to_string()
        })
        .collect()
}

pub(crate) fn welcome(req_id: &str) -> String {
    frame(req_id, "CortexFS is ready.")
}

fn frame(req_id: &str, text: &str) -> String {
    json!({
        "cmd": "aibot_respond_welcome_msg",
        "headers": {"req_id": req_id},
        "body": {"msgtype": "text", "text": {"content": text}}
    })
    .to_string()
}

fn chunks(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if current.len() + character.len_utf8() > MAX_FRAME_BYTES && !current.is_empty() {
            result.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() || result.is_empty() {
        result.push(current);
    }
    result
}

pub(crate) fn reconnect(frame: &Value) -> bool {
    frame
        .get("body")
        .and_then(|body| body.get("event"))
        .and_then(|event| event.get("eventtype"))
        .and_then(Value::as_str)
        == Some("disconnected_event")
}

#[cfg(test)]
mod tests;
