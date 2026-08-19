use crate::{ChannelActions, ChannelCapabilities};

/// Transport family used by a platform adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelTransport {
    Polling,
    Webhook,
    WebSocket,
    Stdio,
    LocalApi,
    External,
}

/// Discoverable platform entry without putting platform fields in Message ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelSpec {
    pub id: &'static str,
    pub transport: ChannelTransport,
    pub native: bool,
    pub capabilities: ChannelCapabilities,
}

/// Common capability tools made available inside every channel tool namespace.
pub const COMMON_CHANNEL_TOOLS: &[&str] = &[
    "channel.send",
    "channel.reply",
    "channel.typing",
    "channel.preview",
    "channel.react",
    "channel.edit",
    "channel.delete",
    "channel.mark_read",
    "channel.pin",
    "channel.unpin",
    "channel.redact",
    "channel.choice",
    "channel.approval",
    "channel.notify",
    "channel.room_create",
    "channel.room_invite",
    "channel.draft",
    "channel.gate",
    "channel.forge",
];

impl ChannelSpec {
    /// Returns common tools plus one namespaced escape hatch for platform APIs.
    #[must_use]
    pub fn tool_names(self) -> Vec<String> {
        COMMON_CHANNEL_TOOLS
            .iter()
            .map(|name| (*name).to_owned())
            .chain(std::iter::once(format!("{}.invoke", self.id)))
            .collect()
    }
}

impl ChannelSpec {
    /// Effects supported by the reusable codec or native host.
    #[must_use]
    pub fn actions(self) -> ChannelActions {
        match self.id {
            "telegram" | "discord" => ChannelActions {
                typing: true,
                reaction: true,
                edit: true,
                delete: true,
                pin: true,
                unpin: true,
                redact: true,
                ..ChannelActions::empty()
            },
            "slack" | "mattermost" => ChannelActions {
                reaction: true,
                edit: true,
                delete: true,
                pin: true,
                unpin: true,
                redact: true,
                ..ChannelActions::empty()
            },
            "matrix" => ChannelActions {
                reaction: true,
                edit: true,
                delete: true,
                mark_read: true,
                redact: true,
                ..ChannelActions::empty()
            },
            _ => ChannelActions::empty(),
        }
    }
}

const TEXT: ChannelCapabilities = ChannelCapabilities::text();
const WEBHOOK: ChannelCapabilities = ChannelCapabilities {
    webhook: true,
    ..TEXT
};
const SEND_WEBHOOK: ChannelCapabilities = ChannelCapabilities {
    send: true,
    webhook: true,
    ..ChannelCapabilities::empty()
};
const LONG_POLL: ChannelCapabilities = ChannelCapabilities {
    polling: true,
    long_polling: true,
    ..TEXT
};
const POLL: ChannelCapabilities = ChannelCapabilities {
    polling: true,
    ..TEXT
};
const VOICE: ChannelCapabilities = ChannelCapabilities {
    audio: true,
    webhook: true,
    ..TEXT
};
const VOICE_WAKE: ChannelCapabilities = ChannelCapabilities {
    audio: true,
    ..ChannelCapabilities::empty()
};
const SOCKET: ChannelCapabilities = ChannelCapabilities {
    websocket: true,
    ..TEXT
};
const TELEGRAM: ChannelCapabilities = ChannelCapabilities {
    group: true,
    threads: true,
    media: true,
    attachments: true,
    send_attachments: true,
    typing: true,
    reactions: true,
    streaming: true,
    draft_updates: true,
    ..LONG_POLL
};
const DISCORD: ChannelCapabilities = ChannelCapabilities {
    group: true,
    threads: true,
    media: true,
    attachments: true,
    receive_attachments: true,
    send_attachments: true,
    typing: true,
    reactions: true,
    streaming: true,
    draft_updates: true,
    ..SOCKET
};
const SLACK: ChannelCapabilities = ChannelCapabilities {
    attachments: true,
    receive_attachments: true,
    send_attachments: true,
    reactions: true,
    commands: true,
    choices: true,
    ..GROUP_WEBHOOK
};
const LINE: ChannelCapabilities = ChannelCapabilities {
    attachments: true,
    send_attachments: true,
    ..GROUP_WEBHOOK
};
const WHATSAPP: ChannelCapabilities = ChannelCapabilities {
    attachments: true,
    send_attachments: true,
    ..WEBHOOK
};
const LINQ: ChannelCapabilities = ChannelCapabilities {
    attachments: true,
    receive_attachments: true,
    send_attachments: true,
    ..WEBHOOK
};
const MATTERMOST: ChannelCapabilities = ChannelCapabilities {
    attachments: true,
    receive_attachments: true,
    send_attachments: true,
    ..GROUP_SOCKET
};
const TEAMS: ChannelCapabilities = ChannelCapabilities {
    media: true,
    attachments: true,
    receive_attachments: true,
    send_attachments: true,
    ..GROUP_WEBHOOK
};
const GROUP_SOCKET: ChannelCapabilities = ChannelCapabilities {
    group: true,
    threads: true,
    ..SOCKET
};
const GROUP_POLL: ChannelCapabilities = ChannelCapabilities {
    group: true,
    threads: true,
    ..LONG_POLL
};
const GROUP_WEBHOOK: ChannelCapabilities = ChannelCapabilities {
    group: true,
    threads: true,
    ..WEBHOOK
};
const BLUESKY: ChannelCapabilities = ChannelCapabilities {
    threads: true,
    ..POLL
};

/// The upstream `ZeroClaw` channel families known to `CortexFS`.
pub const CHANNEL_CATALOG: &[ChannelSpec] = &[
    spec("telegram", ChannelTransport::Polling, true, TELEGRAM),
    spec("discord", ChannelTransport::WebSocket, true, DISCORD),
    spec("bluesky", ChannelTransport::Polling, true, BLUESKY),
    spec("slack", ChannelTransport::Webhook, true, SLACK),
    spec("feishu", ChannelTransport::Webhook, true, GROUP_WEBHOOK),
    spec("dingtalk", ChannelTransport::WebSocket, true, GROUP_SOCKET),
    spec("line", ChannelTransport::Webhook, true, LINE),
    spec("lark", ChannelTransport::Webhook, true, GROUP_WEBHOOK),
    spec("teams", ChannelTransport::Webhook, true, TEAMS),
    spec(
        "nextcloud_talk",
        ChannelTransport::Webhook,
        true,
        GROUP_WEBHOOK,
    ),
    spec("matrix", ChannelTransport::Polling, true, GROUP_POLL),
    spec("mattermost", ChannelTransport::WebSocket, true, MATTERMOST),
    spec("qq", ChannelTransport::WebSocket, true, GROUP_SOCKET),
    spec("reddit", ChannelTransport::Polling, true, POLL),
    spec("signal", ChannelTransport::LocalApi, true, TEXT),
    spec("irc", ChannelTransport::Polling, true, LONG_POLL),
    spec("twitch", ChannelTransport::Polling, true, LONG_POLL),
    spec("email", ChannelTransport::Polling, true, LONG_POLL),
    spec("gmail", ChannelTransport::Webhook, true, WEBHOOK),
    spec("whatsapp", ChannelTransport::Webhook, true, WHATSAPP),
    spec("wecom", ChannelTransport::Webhook, true, SEND_WEBHOOK),
    spec("whatsapp_web", ChannelTransport::External, false, TEXT),
    spec("imessage", ChannelTransport::External, false, TEXT),
    spec("nostr", ChannelTransport::External, false, SOCKET),
    spec("twitter", ChannelTransport::Polling, true, POLL),
    spec("mochat", ChannelTransport::Polling, true, POLL),
    spec("linq", ChannelTransport::Webhook, true, LINQ),
    spec("notion", ChannelTransport::Polling, true, POLL),
    spec("wechat", ChannelTransport::Polling, false, LONG_POLL),
    spec("wecom_ws", ChannelTransport::External, false, SOCKET),
    spec("clawdtalk", ChannelTransport::External, false, VOICE),
    spec("voice_call", ChannelTransport::External, false, VOICE),
    spec("voice_wake", ChannelTransport::External, false, VOICE_WAKE),
    spec("webhook", ChannelTransport::Webhook, true, WEBHOOK),
    spec("cli", ChannelTransport::Stdio, true, TEXT),
    spec("acp", ChannelTransport::Stdio, false, TEXT),
    spec("filesystem", ChannelTransport::LocalApi, false, TEXT),
    spec("git", ChannelTransport::LocalApi, false, TEXT),
    spec("amqp", ChannelTransport::External, false, TEXT),
    spec("mqtt", ChannelTransport::External, false, TEXT),
];

#[must_use]
pub fn find(id: &str) -> Option<&'static ChannelSpec> {
    let family = id.split_once('.').map_or(id, |(family, _)| family);
    CHANNEL_CATALOG.iter().find(|spec| spec.id == family)
}

const fn spec(
    id: &'static str,
    transport: ChannelTransport,
    native: bool,
    capabilities: ChannelCapabilities,
) -> ChannelSpec {
    ChannelSpec {
        id,
        transport,
        native,
        capabilities,
    }
}
