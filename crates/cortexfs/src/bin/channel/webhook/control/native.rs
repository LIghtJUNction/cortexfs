use std::collections::BTreeMap;

use cortexfs_channels::{MessageTarget, OutboundRequest};
use serde_json::{Value, json};

use super::super::super::config::Platform;
use cortexfs::channel::control::ChannelControlError;

pub(super) fn request(
    platform: Platform,
    target: &MessageTarget,
    name: &str,
    payload: &Value,
) -> Result<OutboundRequest, ChannelControlError> {
    let conversation = target.conversation.as_str();
    let (path, body) = match (platform, name) {
        (
            Platform::Feishu,
            "feishu.send_card" | "lark.send_card" | "feishu.send_file" | "lark.send_file",
        ) => (
            "im/v1/messages".to_owned(),
            json!({"receive_id":conversation,"msg_type":"interactive","content":payload}),
        ),
        (
            Platform::Feishu,
            "feishu.send_post" | "lark.send_post" | "feishu.update_message" | "lark.update_message",
        ) => (
            "im/v1/messages".to_owned(),
            json!({"receive_id":conversation,"msg_type":"post","content":payload}),
        ),
        (
            Platform::Feishu,
            "feishu.send_image" | "lark.send_image" | "feishu.reaction" | "lark.reaction",
        ) => (
            "im/v1/messages".to_owned(),
            json!({"receive_id":conversation,"msg_type":"image","content":payload}),
        ),
        (
            Platform::Line,
            "line.push" | "line.reply" | "line.send_template" | "line.send_flex"
            | "line.send_image",
        ) => (
            "v2/bot/message/push".to_owned(),
            json!({"to":conversation,"messages":payload.get("messages").cloned().unwrap_or(json!([]))}),
        ),
        (
            Platform::Teams,
            "teams.send_activity"
            | "teams.send_adaptive_card"
            | "teams.upload_file"
            | "teams.update_activity",
        ) => (
            format!("v3/conversations/{conversation}/activities"),
            payload.clone(),
        ),
        (
            Platform::Nextcloud,
            "nextcloud_talk.send_file"
            | "nextcloud_talk.draft"
            | "nextcloud_talk.update_draft"
            | "nextcloud_talk.finalize_draft",
        ) => (
            format!("ocs/v2.php/apps/spreed/api/v1/bot/{conversation}/message?format=json"),
            json!({"message":payload.get("text").cloned().unwrap_or(Value::String(String::new())),"file":payload.get("file")}),
        ),
        (Platform::Linq, "linq.send_url" | "linq.send_media" | "linq.send_reaction") => (
            format!("chats/{conversation}/messages"),
            json!({"message":{"parts":[payload]}}),
        ),
        (
            Platform::WhatsApp,
            "whatsapp.send_template"
            | "whatsapp_web.send_media"
            | "whatsapp_web.send_location"
            | "whatsapp_web.send_interactive"
            | "whatsapp_web.request_approval"
            | "whatsapp.send_interactive"
            | "whatsapp.send_location"
            | "whatsapp.send_media"
            | "whatsapp.send_document"
            | "whatsapp.request_approval",
        ) => ("messages".to_owned(), payload.clone()),
        (
            Platform::WeCom,
            "wecom.send_markdown"
            | "wecom.send_news"
            | "wecom.send_template_card"
            | "wecom.send_image"
            | "wecom.send_file"
            | "wecom.draft_update",
        ) => ("send".to_owned(), payload.clone()),
        _ => return Err(fail("unsupported operation")),
    };
    Ok(OutboundRequest {
        method: "POST".to_owned(),
        path,
        content_type: "application/json".to_owned(),
        body: body.to_string(),
        headers: BTreeMap::new(),
    })
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
