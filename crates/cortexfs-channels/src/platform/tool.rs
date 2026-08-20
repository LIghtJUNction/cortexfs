/// Names of adapter-owned operations exposed as channel-local tools.
///
/// The operation names are strings on purpose: payloads remain adapter-owned
/// and never become platform-specific `CortexFS` message types.
pub(super) fn names(id: &str) -> Vec<String> {
    operations(id)
        .split_whitespace()
        .map(|name| format!("{id}.{name}"))
        .collect()
}

fn operations(id: &str) -> &'static str {
    TOOLSETS
        .iter()
        .find(|entry| entry.0 == id)
        .map_or("", |entry| entry.1)
}

#[rustfmt::skip]
const TOOLSETS: &[(&str, &str)] = &[
    ("telegram", "send_photo send_document send_video send_audio send_voice send_location send_poll answer_callback draft_update"),
    ("discord", "send_embed send_file create_thread send_component register_command autocomplete gate_prompt gate_finalize draft_update"),
    ("slack", "send_blocks upload_file post_ephemeral open_modal list_channels thread_reply draft_update"),
    ("feishu", "send_card send_post send_image send_file update_message reaction"),
    ("lark", "send_card send_post send_image send_file update_message reaction"),
    ("dingtalk", "send_markdown send_action_card send_image send_file sign_request"),
    ("line", "push reply send_template send_flex send_image"),
    ("teams", "send_adaptive_card send_activity upload_file update_activity"),
    ("nextcloud_talk", "send_file draft update_draft finalize_draft"),
    ("matrix", "send_html upload_media create_room join_room invite_user redact_event send_reaction read_receipt"),
    ("mattermost", "post upload thread_reply add_reaction remove_reaction pin_post unpin_post"),
    ("qq", "send_markdown send_media send_keyboard send_group send_c2c"),
    ("reddit", "submit_post comment reply edit delete vote flair"),
    ("signal", "send_poll send_attachment send_reaction remove_reaction request_approval"),
    ("irc", "raw join part notice action topic"),
    ("twitch", "send_whisper set_title set_game timeout ban"),
    ("email", "search read reply forward send_attachment mark_read"),
    ("gmail", "search read reply forward register_watch fetch_history fetch_message"),
    ("whatsapp", "send_template send_interactive send_location send_media send_document request_approval"),
    ("whatsapp_web", "send_media send_location send_interactive request_approval"),
    ("wecom", "send_markdown send_news send_template_card send_image send_file draft_update"),
    ("wecom_ws", "send_markdown send_media send_file draft_update"),
    ("bluesky", "create_post reply like unlike repost follow quote"),
    ("twitter", "post reply send_dm search_mentions like"),
    ("mochat", "send_media update cursor"),
    ("linq", "send_url send_media send_reaction"),
    ("notion", "query_database create_page update_page append_block"),
    ("wechat", "send_markdown send_media send_file start_typing draft_update"),
    ("clawdtalk", "start_call speak hangup transfer"),
    ("voice_call", "start_call speak hangup transfer record"),
    ("voice_wake", "wake stop"),
    ("webhook", "send challenge health"),
    ("cli", "write read"),
    ("acp", "request_input request_choice request_permission send_chunk"),
    ("filesystem", "append read write"),
    ("git", "forge_request issue pull_request comment labels approve"),
    ("amqp", "publish subscribe ack reject"),
    ("mqtt", "publish subscribe ack reject"),
    ("imessage", "send_attachment send_group send_effect"),
    ("nostr", "publish send_dm query_relays reaction"),
];
