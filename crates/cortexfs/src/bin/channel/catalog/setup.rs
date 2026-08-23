#[derive(Clone, Copy)]
pub(super) struct Setup {
    pub(super) id: &'static str,
    pub(super) command: &'static str,
    pub(super) unit: &'static str,
    pub(super) secrets: &'static [&'static str],
}

const fn setup(
    id: &'static str,
    command: &'static str,
    unit: &'static str,
    secrets: &'static [&'static str],
) -> Setup {
    Setup {
        id,
        command,
        unit,
        secrets,
    }
}

#[rustfmt::skip]
pub(super) const SETUPS: &[Setup] = &[
    setup("telegram", "cortexfs-channel telegram", "cortexfs-channel-telegram.service", &["CORTEXFS_TELEGRAM_TOKEN"]),
    setup("discord", "cortexfs-channel discord", "cortexfs-channel@discord.service", &["application_id", "bot_token"]),
    setup("bluesky", "cortexfs-channel bluesky", "cortexfs-channel-bluesky.service", &["CORTEXFS_BLUESKY_HANDLE", "CORTEXFS_BLUESKY_APP_PASSWORD"]),
    setup("dingtalk", "cortexfs-channel dingtalk", "cortexfs-channel-dingtalk.service", &["CORTEXFS_DINGTALK_CLIENT_ID", "CORTEXFS_DINGTALK_CLIENT_SECRET"]),
    setup("matrix", "cortexfs-channel matrix", "cortexfs-channel-matrix.service", &["CORTEXFS_MATRIX_HOMESERVER", "CORTEXFS_MATRIX_ACCESS_TOKEN"]),
    setup("mattermost", "cortexfs-channel mattermost", "cortexfs-channel-mattermost.service", &["CORTEXFS_MATTERMOST_URL", "CORTEXFS_MATTERMOST_TOKEN"]),
    setup("qq", "cortexfs-channel qq", "cortexfs-channel-qq.service", &["CORTEXFS_QQ_APP_ID", "CORTEXFS_QQ_TOKEN"]),
    setup("reddit", "cortexfs-channel reddit", "cortexfs-channel-reddit.service", &["CORTEXFS_REDDIT_CLIENT_ID", "CORTEXFS_REDDIT_CLIENT_SECRET", "CORTEXFS_REDDIT_REFRESH_TOKEN", "CORTEXFS_REDDIT_USERNAME"]),
    setup("gmail", "cortexfs-channel gmail", "cortexfs-channel-gmail.service", &["CORTEXFS_GMAIL_ACCESS_TOKEN"]),
    setup("email", "cortexfs-channel email", "cortexfs-channel-email.service", &["CORTEXFS_EMAIL_IMAP_HOST", "CORTEXFS_EMAIL_SMTP_HOST", "CORTEXFS_EMAIL_USERNAME", "CORTEXFS_EMAIL_PASSWORD"]),
    setup("irc", "cortexfs-channel irc", "cortexfs-channel-irc.service", &["CORTEXFS_IRC_SERVER", "CORTEXFS_IRC_NICKNAME"]),
    setup("twitch", "cortexfs-channel twitch", "cortexfs-channel-twitch.service", &["CORTEXFS_TWITCH_USERNAME", "CORTEXFS_TWITCH_OAUTH_TOKEN", "CORTEXFS_TWITCH_CHANNELS"]),
    setup("twitter", "cortexfs-channel twitter", "cortexfs-channel-twitter.service", &["CORTEXFS_TWITTER_BEARER_TOKEN"]),
    setup("mochat", "cortexfs-channel mochat", "cortexfs-channel-mochat.service", &["CORTEXFS_MOCHAT_API_BASE", "CORTEXFS_MOCHAT_API_TOKEN"]),
    setup("notion", "cortexfs-channel notion", "cortexfs-channel-notion.service", &["CORTEXFS_NOTION_API_TOKEN", "CORTEXFS_NOTION_DATABASE_ID"]),
    setup("signal", "cortexfs-channel signal", "cortexfs-channel-signal.service", &["CORTEXFS_SIGNAL_ACCOUNT"]),
    setup("webhook", "cortexfs-channel webhook", "cortexfs-channel-webhook.service", &["CORTEXFS_CHANNEL_PLATFORM", "CORTEXFS_CHANNEL_OUTBOUND_URL"]),
    setup("web", "cortexfs-channel web", "cortexfs-channel-web.service", &[]),
    setup("slack", "cortexfs-channel webhook", "cortexfs-channel-slack.service", &["CORTEXFS_CHANNEL_PLATFORM=slack", "CORTEXFS_CHANNEL_TOKEN", "CORTEXFS_CHANNEL_OUTBOUND_URL"]),
    setup("feishu", "cortexfs-channel webhook", "cortexfs-channel-feishu.service", &["CORTEXFS_CHANNEL_PLATFORM=feishu", "CORTEXFS_CHANNEL_TOKEN", "CORTEXFS_CHANNEL_OUTBOUND_URL"]),
    setup("lark", "cortexfs-channel webhook", "cortexfs-channel-lark.service", &["CORTEXFS_CHANNEL_PLATFORM=lark", "CORTEXFS_CHANNEL_TOKEN", "CORTEXFS_CHANNEL_OUTBOUND_URL"]),
    setup("line", "cortexfs-channel webhook", "cortexfs-channel-line.service", &["CORTEXFS_CHANNEL_PLATFORM=line", "CORTEXFS_CHANNEL_TOKEN", "CORTEXFS_CHANNEL_OUTBOUND_URL"]),
    setup("teams", "cortexfs-channel webhook", "cortexfs-channel-teams.service", &["CORTEXFS_CHANNEL_PLATFORM=teams", "CORTEXFS_CHANNEL_TOKEN", "CORTEXFS_CHANNEL_OUTBOUND_URL"]),
    setup("whatsapp", "cortexfs-channel webhook", "cortexfs-channel-whatsapp.service", &["CORTEXFS_CHANNEL_PLATFORM=whatsapp", "CORTEXFS_CHANNEL_TOKEN", "CORTEXFS_CHANNEL_OUTBOUND_URL"]),
    setup("wecom", "cortexfs-channel webhook", "cortexfs-channel-wecom.service", &["CORTEXFS_CHANNEL_PLATFORM=wecom", "CORTEXFS_CHANNEL_OUTBOUND_URL"]),
    setup("nostr", "cortexfs-channel-nostr", "cortexfs-channel-nostr.service", &["CORTEXFS_NOSTR_NSEC"]),
    setup("wechat", "cortexfs-channel-wechat", "cortexfs-channel-wechat.service", &["CORTEXFS_WECHAT_TOKEN"]),
    setup("wecom-ws", "cortexfs-channel-wecom-ws", "cortexfs-channel-wecom-ws.service", &["CORTEXFS_WECOM_WS_BOT_ID", "CORTEXFS_WECOM_WS_SECRET"]),
    setup("amqp", "cortexfs-channel-amqp", "cortexfs-channel-amqp.service", &["CORTEXFS_AMQP_URL"]),
    setup("mqtt", "cortexfs-channel-mqtt", "cortexfs-channel-mqtt.service", &["CORTEXFS_MQTT_URL"]),
];

pub(super) fn lookup(id: &str) -> Option<&'static Setup> {
    SETUPS.iter().find(|setup| setup.id == id)
}
