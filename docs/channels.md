---
sidebar_position: 5
---

# Multi-IM channels

CortexFS exposes IM as a transport around the existing agent session, not as a
new filesystem namespace. The reusable layer is the publishable
`cortexfs-channels` crate; CortexFS adds a small bridge and a foreground host.
The crate is published at [crates.io](https://crates.io/crates/cortexfs-channels)
and its API is browsable on [docs.rs](https://docs.rs/cortexfs-channels).

## Install and start

After installing the normal CortexFS package, start an agent and write the
Discord adapter configuration to one owner-only file:

```bash
ctx agent start coder --session default
sudo install -d -m 0700 /etc/cortexfs/channels
sudoedit /etc/cortexfs/channels/discord.toml
sudo chmod 600 /etc/cortexfs/channels/discord.toml
```

The file contains the application id, bot token, agent socket path, and agent
name. The token is read only from this file and is redacted from diagnostics:

```toml
application_id = "DISCORD_APPLICATION_ID"
bot_token = "DISCORD_BOT_TOKEN"
agent_socket = "/ctx/agent/main.sock"
agent = "main"
session_prefix = "discord"
# Optional complete instance id; omit for the base `discord` id.
# channel = "discord.primary"

# Optional progress presentation. Omit a value to disable that effect.
[progress]
reaction = "👀"
error_reaction = "❌"
placeholder = "⏳ 思考中…"
error_prefix = "⚠️ "
typing = true
edit_interval_ms = 700
edit_chunk_bytes = 512
```

The progress values are configuration, not Discord ABI constants. The same
policy can use any reaction or text, or disable the preview entirely by
omitting the corresponding values. Without `progress.placeholder`, CortexFS
sends the final reply normally and does not create a temporary message.

Start the low-memory synchronous Gateway adapter after enabling the Discord
`MESSAGE_CONTENT` privileged intent in the Discord Developer Portal:

```bash
sudo systemctl enable --now cortexfs-channel@discord.service
sudo journalctl -u cortexfs-channel@discord.service -f
```

The adapter keeps one bounded WebSocket connection and uses the public
`/ctx/agent/main.sock` alias for the existing durable session ABI. The default
reference tree materializes `agent/main -> agent/coder` and
`agent/main.sock -> agent/coder.sock`; the canonical control and session owner
remains `coder`.
It validates that the endpoint is a live Unix socket before connecting. Channel
state and tools are visible under `/ctx/channel/discord/` and
`/ctx/channel/discord.d/`; credentials remain outside `/ctx`. There is no
background watcher or hot-reload path.

### How an Agent controls Discord

An inbound Discord message follows this path:

```text
Discord Gateway
    -> cortexfs-channel (Discord host)
    -> AgentChannelBridge
    -> existing agent session socket
    -> Agent model/tool call
```

When the Agent calls `discord.send_embed`, `discord.send_file`, or another
Discord-local tool, the tool executable does not receive the bot token and does
not call Discord directly. It writes a provider-neutral
`ChannelCommand::Invoke { name, payload }` request to the Discord channel
driver socket. The driver checks the negotiated `tool_control` capability and
forwards the command to the running Discord host. The host validates the
payload, calls the Discord REST API with its private bot token, and returns a
correlated `CommandResult`.

For example, an Agent can target the conversation from the inbound message with
the following operation payload:

```json
{
  "name": "discord.send_embed",
  "payload": {
    "title": "Build complete",
    "description": "The release checks passed.",
    "color": 5763719
  }
}
```

`channel.send` uses the same driver path for ordinary text; `channel.react`,
`channel.edit`, `channel.pin`, and typing/preview effects use the
provider-neutral `Outbound`/`Effect` frames. Thus the Agent controls the
Discord account through the already-running host and its capability-gated
Unix socket, not by being given Discord credentials.

To run more than one account of the same platform, give each foreground host
an instance id, for example `CORTEXFS_CHANNEL_ID=telegram.primary` and
`CORTEXFS_CHANNEL_ID=telegram.secondary`. The complete id is used for channel
and session routing; this does not require one TCP port per account.

After changing a provider or model, refresh the durable backing generation
before starting the channel:

```bash
sudo ctx storage update --prune /var/lib/cortexfs/storage
sudo systemctl restart cortexfs.service
sudo systemctl restart cortexfs-agent@coder.socket
sudo systemctl restart cortexfs-channel@discord.service
sudo ctx doctor
```

The generation refresh is required because `/ctx` is a projection while the
agent runtime reads the selected backing generation directly. A projected
model entry alone is not proof that the runtime can resolve it.

Telegram uses long polling:

```bash
export CORTEXFS_TELEGRAM_TOKEN='...'
cortexfs-channel telegram
```

Telegram's optional progress presentation uses the same generic environment
variables: `CORTEXFS_CHANNEL_PROGRESS_REACTION`,
`CORTEXFS_CHANNEL_PROGRESS_ERROR_REACTION`,
`CORTEXFS_CHANNEL_PROGRESS_PLACEHOLDER`,
`CORTEXFS_CHANNEL_PROGRESS_ERROR_PREFIX`,
`CORTEXFS_CHANNEL_PROGRESS_TYPING`,
`CORTEXFS_CHANNEL_PROGRESS_EDIT_INTERVAL_MS`, and
`CORTEXFS_CHANNEL_PROGRESS_EDIT_CHUNK_BYTES`.

Bluesky uses the AT Protocol notification API with an app password. It polls
mentions and replies, maps the sender DID through the same identity-isolated
session route, and creates threaded `app.bsky.feed.post` replies. Credentials
remain in the service environment; they are never written to `/ctx` or
session history:

```bash
export CORTEXFS_BLUESKY_HANDLE='agent.example'
export CORTEXFS_BLUESKY_APP_PASSWORD='read-from-a-secret-store'
cortexfs-channel bluesky
```

For a packaged deployment, put those values in
`/etc/cortexfs/channels/bluesky.env` with mode `0600` and run
`sudo systemctl enable --now cortexfs-channel-bluesky.service`. The host
refreshes its short-lived AT Protocol session after reconnects and marks only
successfully handled notifications as seen.

DingTalk uses the official Stream Mode gateway. Keep both credentials in the
service manager's secret environment and start one foreground host per agent:

```bash
export CORTEXFS_DINGTALK_CLIENT_ID='...'
export CORTEXFS_DINGTALK_CLIENT_SECRET='...'
cortexfs-channel dingtalk
```

For a packaged deployment, put the complete runtime configuration in the
owner-only environment file and let systemd supervise reconnects:

```bash
sudo install -m 600 /dev/null /etc/cortexfs/channels/dingtalk.env
sudoedit /etc/cortexfs/channels/dingtalk.env
sudo systemctl enable --now cortexfs-channel-dingtalk.service
```

The file must contain `CORTEXFS_AGENT`, its canonical
`CORTEXFS_AGENT_SOCKET`, `CORTEXFS_DINGTALK_CLIENT_ID`, and
`CORTEXFS_DINGTALK_CLIENT_SECRET`. Optional values include
`CORTEXFS_CHANNEL_SESSION_PREFIX`, `CORTEXFS_AGENT_CWD`, and
`CORTEXFS_DINGTALK_GATEWAY_URL`.

The host acknowledges gateway frames, reconnects after a dropped WebSocket,
maps private and group conversations through the generic identity-isolated
route, and sends Markdown replies through DingTalk's per-message session
webhook. The session webhook is transient channel state; it is never written
to `messages.jsonl`, `events.jsonl`, or `/ctx`.

Matrix uses the Client-Server API with a bearer access token. The adapter calls
`whoami`, then maintains a bounded long-poll `/sync` cursor and sends text
replies with Matrix reply/thread relations:

```bash
export CORTEXFS_MATRIX_HOMESERVER='https://matrix.example.org'
export CORTEXFS_MATRIX_ACCESS_TOKEN='...'
# Optional comma-separated room IDs:
export CORTEXFS_MATRIX_ROOMS='!room:example.org'
cortexfs-channel matrix
```

For a packaged service, add these variables to
`/etc/cortexfs/channels/matrix.env` with mode `0600`, then run
`sudo systemctl enable --now cortexfs-channel-matrix.service`. The access
token is not stored in `/ctx` or the session files.

Mattermost uses its native WebSocket event stream and REST post API. URL-backed
attachments are mapped through Mattermost post properties; raw file upload is
still adapter-owned. Set an
owner-only environment file for a packaged host:

```bash
sudo install -m 600 /dev/null /etc/cortexfs/channels/mattermost.env
sudoedit /etc/cortexfs/channels/mattermost.env
sudo systemctl enable --now cortexfs-channel-mattermost.service
```

The file requires `CORTEXFS_AGENT`, `CORTEXFS_AGENT_SOCKET`,
`CORTEXFS_MATTERMOST_URL`, and `CORTEXFS_MATTERMOST_TOKEN`. Optionally set
`CORTEXFS_MATTERMOST_CHANNELS` to a comma-separated channel-id allowlist and
`CORTEXFS_MATTERMOST_RECONNECT_SECONDS`. Mattermost thread replies are mapped
to the generic `thread` field. URL-backed attachments remain in the generic
attachment list and are emitted through post properties.

QQ uses the Bot API Gateway for inbound guild, group, and C2C events and the
corresponding REST endpoints for replies. Configure an owner-only environment
file for the packaged host:

```bash
sudo install -m 600 /dev/null /etc/cortexfs/channels/qq.env
sudoedit /etc/cortexfs/channels/qq.env
sudo systemctl enable --now cortexfs-channel-qq.service
```

The file requires `CORTEXFS_AGENT`, `CORTEXFS_AGENT_SOCKET`,
`CORTEXFS_QQ_APP_ID`, and `CORTEXFS_QQ_TOKEN`. Optional values are
`CORTEXFS_QQ_INTENTS`, `CORTEXFS_QQ_API_BASE`, `CORTEXFS_QQ_GATEWAY_URL`, and
`CORTEXFS_QQ_RECONNECT_SECONDS`. Guild, group, and C2C targets stay in generic
message metadata so the core message ABI does not gain QQ-specific fields;
media attachments are rejected until a bounded upload policy is configured.

Gmail Push uses a small Pub/Sub callback listener and the Gmail history API.
The listener only receives a history cursor; it then fetches message data with
the bearer token and routes each sender through the same identity-isolated
session bridge:

```bash
export CORTEXFS_GMAIL_ACCESS_TOKEN='...'
export CORTEXFS_GMAIL_BIND='127.0.0.1:8767'
export CORTEXFS_GMAIL_PATH='/gmail/push'
cortexfs-channel gmail
```

Email uses IMAP IDLE for inbound mail and SMTP STARTTLS for replies. The
current host handles plain text and simple RFC 5322 messages. Outbound MIME
attachments are available through `email.send_attachment`, bounded to 8 MiB;
inbound attachment download remains adapter-specific:

```bash
export CORTEXFS_EMAIL_IMAP_HOST='imap.example.org'
export CORTEXFS_EMAIL_SMTP_HOST='smtp.example.org'
export CORTEXFS_EMAIL_USERNAME='agent@example.org'
export CORTEXFS_EMAIL_PASSWORD='read-from-a-secret-store'
cortexfs-channel email
```

IRC uses a reconnecting TCP client with `PRIVMSG`, private conversations, and
configured channel joins. The current transport is plain IRC; use a TLS relay
or an encrypted network endpoint when the server requires confidentiality:

```bash
export CORTEXFS_IRC_SERVER='irc.example.org'
export CORTEXFS_IRC_NICKNAME='cortexfs-agent'
export CORTEXFS_IRC_CHANNELS='#agents'
cortexfs-channel irc
```

Twitch uses its TLS IRC endpoint with the same generic IRC stream runner. The
OAuth token is normalized to the required `oauth:` password form, and channel
names are normalized to lower-case `#name` targets. Set mention-only mode when
the bot should ignore messages that do not address its username:

```bash
export CORTEXFS_TWITCH_USERNAME='cortexfs-agent'
export CORTEXFS_TWITCH_OAUTH_TOKEN='read-from-a-secret-store'
export CORTEXFS_TWITCH_CHANNELS='#agents,other-channel'
export CORTEXFS_TWITCH_MENTION_ONLY=true
cortexfs-channel twitch
```

For a packaged deployment, put the values in
`/etc/cortexfs/channels/twitch.env` with mode `0600`, then run
`sudo systemctl enable --now cortexfs-channel-twitch.service`. The default
endpoint is `irc.chat.twitch.tv:6697`; the host verifies the server certificate
using the bundled public root set and reconnects after a dropped stream.

Reddit uses OAuth2 refresh-token authentication and polls the unread inbox for
mentions, direct messages, and comment replies. Public replies use the Reddit
thing fullname (`t1_`, `t3_`, or `t4_`) as the generic reply target; direct
messages use the sender identity. Successfully handled inbox items are marked
read only after their agent response is delivered:

```bash
export CORTEXFS_REDDIT_CLIENT_ID='read-from-a-secret-store'
export CORTEXFS_REDDIT_CLIENT_SECRET='read-from-a-secret-store'
export CORTEXFS_REDDIT_REFRESH_TOKEN='read-from-a-secret-store'
export CORTEXFS_REDDIT_USERNAME='cortexfs-agent'
export CORTEXFS_REDDIT_SUBREDDITS='rust,linux'
cortexfs-channel reddit
```

For a packaged deployment, put the values in
`/etc/cortexfs/channels/reddit.env` with mode `0600`, then run
`sudo systemctl enable --now cortexfs-channel-reddit.service`. The OAuth
token is refreshed in memory and never enters `/ctx`, session metadata, or
diagnostic output.

WeCom Bot Webhook is exposed through the same generic webhook host. Its
official Bot Webhook surface is send-only, so CortexFS declares
`receive=false` instead of pretending that callback JSON can produce inbound
messages. Put the webhook key in the outbound URL and do not configure a
bearer token:

```bash
export CORTEXFS_CHANNEL_PLATFORM=wecom
export CORTEXFS_CHANNEL_BIND=127.0.0.1:8765
export CORTEXFS_CHANNEL_OUTBOUND_URL='https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=read-from-a-secret-store'
cortexfs-channel webhook
```

Mochat uses its HTTP receive/send API with a monotonic message cursor. The
host routes each accepted sender through the same identity-isolated session
mapper and never stores the bearer token in `/ctx`. An empty sender allowlist
denies all inbound users:

```bash
export CORTEXFS_MOCHAT_API_BASE='https://mochat.example/api'
export CORTEXFS_MOCHAT_API_TOKEN='read-from-a-secret-store'
export CORTEXFS_MOCHAT_ALLOWED_USERS='user-1,user-2'
cortexfs-channel mochat
```

For a packaged deployment, put the values in
`/etc/cortexfs/channels/mochat.env` with mode `0600`, then run
`sudo systemctl enable --now cortexfs-channel-mochat.service`.

Linq uses the Partner API webhook. Configure the callback through the generic
webhook host; the optional verify token is used as the HMAC secret for the
`x-linq-timestamp`/`x-linq-signature` headers. The outbound URL should point at
the Linq API base and contain `{path}` so replies can target the inbound chat:

```bash
export CORTEXFS_CHANNEL_PLATFORM=linq
export CORTEXFS_CHANNEL_VERIFY_TOKEN='read-from-a-secret-store'
export CORTEXFS_CHANNEL_TOKEN='read-from-a-secret-store'
export CORTEXFS_CHANNEL_OUTBOUND_URL='https://api.linqapp.com/api/partner/v3/{path}'
cortexfs-channel webhook
```

Linq text and image/media URLs are normalized into the generic message body;
unsupported media is left to the platform capability boundary. An inbound
message sent by the bot is ignored, and stale signed callbacks are rejected.

Notion maps a database row to one durable CortexFS conversation. Rows whose
status is `pending` are claimed as `running`, their input property is routed
through the agent socket, and the final reply is written to the result
property before the row becomes `done`. The host can recover rows left in
`running` after a crash:

```bash
export CORTEXFS_NOTION_API_TOKEN='read-from-a-secret-store'
export CORTEXFS_NOTION_DATABASE_ID='read-from-a-secret-store'
export CORTEXFS_NOTION_STATUS_PROPERTY='Status'
export CORTEXFS_NOTION_INPUT_PROPERTY='Input'
export CORTEXFS_NOTION_RESULT_PROPERTY='Result'
cortexfs-channel notion
```

For a packaged deployment, put the values in
`/etc/cortexfs/channels/notion.env` with mode `0600`, then run
`sudo systemctl enable --now cortexfs-channel-notion.service`.

Nostr is an isolated external driver because NIP-04/NIP-17 encryption and the
relay WebSocket pool are not part of the low-memory core host. Configure the
agent-facing runtime driver separately from the Nostr secret file:

```bash
sudo install -d -m 0700 /etc/cortexfs/channels
sudoedit /etc/cortexfs/channels/nostr-driver.env
sudo chmod 600 /etc/cortexfs/channels/nostr-driver.env
```

The driver file selects the ordinary Agent and never contains Nostr keys:

```dotenv
CORTEXFS_AGENT=coder
CORTEXFS_AGENT_SOCKET=/ctx/agent/coder.sock
CORTEXFS_CHANNEL_SESSION_PREFIX=nostr
```

Put the Nostr private key, relay list, and explicit sender allowlist in the
separate owner-only `nostr.env` file. The allowlist accepts `npub` or hex public
keys; an empty or missing allowlist denies every inbound sender:

```dotenv
CORTEXFS_NOSTR_PRIVATE_KEY=read-from-a-secret-store
CORTEXFS_NOSTR_RELAYS=wss://relay.example.org,wss://relay.damus.io
CORTEXFS_NOSTR_ALLOWED_USERS=npub1...,hex-public-key
CORTEXFS_NOSTR_REPLY_TIMEOUT_SECONDS=600
```

Start the runtime driver and the isolated Nostr process. Both sides use the
canonical `/run/cortexfs/channel/nostr.sock` path through
`cortexfs_paths::channel_driver_socket`; no per-user TCP port is allocated:

```bash
sudo systemctl enable --now cortexfs-channel-nostr.service
sudo journalctl -u cortexfs-channel-nostr.service -f
```

The driver accepts NIP-04 encrypted direct messages and NIP-17 gift wraps,
maps each allowed sender to one identity-isolated session, and sends text
replies using the same protocol that arrived. It advertises text and WebSocket
capabilities only; attachments, typing, reactions, and interactive approval
commands are not silently emulated. A runtime command is explicitly rejected
when the platform cannot return an interactive answer.

WeCom AI Bot WebSocket is an independent driver because its subscription
secret, WebSocket heartbeat, streaming response frames, and platform access
policy do not belong in the low-memory core. Keep the runtime driver settings
and WeCom credentials in separate owner-only files:

```dotenv
# /etc/cortexfs/channels/wecom-ws-driver.env, mode 0600
CORTEXFS_AGENT=coder
CORTEXFS_AGENT_SOCKET=/ctx/agent/coder.sock
CORTEXFS_CHANNEL_SESSION_PREFIX=wecom-ws
```

```dotenv
# /etc/cortexfs/channels/wecom-ws.env, mode 0600
CORTEXFS_WECOM_BOT_ID=read-from-a-secret-store
CORTEXFS_WECOM_SECRET=read-from-a-secret-store
CORTEXFS_WECOM_ALLOWED_USERS=user-a,user-b
CORTEXFS_WECOM_ALLOWED_GROUPS=chat-a
CORTEXFS_WECOM_REPLY_TIMEOUT_SECONDS=600
```

Start the canonical runtime socket and WebSocket process together:

```bash
sudo systemctl enable --now cortexfs-channel-wecom-ws.service
sudo journalctl -u cortexfs-channel-wecom-ws.service -f
```

The driver connects to WeCom's `openws.work.weixin.qq.com`, subscribes with
the bot id and secret, sends a bounded `ping` heartbeat, reconnects with
exponential backoff, and maps text callbacks into private or group sessions.
An empty allowlist denies every sender. Replies use WeCom stream frames and
preserve the callback request id through generic message metadata, so the
request id does not fragment the durable multi-turn session. Media, voice
transcription, reactions, and interactive approval commands are rejected or
left to explicit future capabilities rather than being presented as text.

WeChat personal iLink Bot is an independent long-polling driver. It is kept
outside the core host because its bot token, `X-WECHAT-UIN` request identity,
cursor, and context token are transport state rather than message ABI fields.
Configure the runtime-facing socket separately from the owner-only driver
environment:

```dotenv
# /etc/cortexfs/channels/wechat-driver.env, mode 0600
CORTEXFS_AGENT=coder
CORTEXFS_AGENT_SOCKET=/ctx/agent/coder.sock
CORTEXFS_CHANNEL_SESSION_PREFIX=wechat
```

```dotenv
# /etc/cortexfs/channels/wechat.env, mode 0600
CORTEXFS_WECHAT_TOKEN=read-from-a-secret-store
CORTEXFS_WECHAT_ALLOWED_USERS=wx-user-a,wx-user-b
CORTEXFS_WECHAT_REPLY_TIMEOUT_SECONDS=600
```

Enable the canonical runtime socket and WeChat process together:

```bash
sudo systemctl enable --now cortexfs-channel-wechat.service
sudo journalctl -u cortexfs-channel-wechat.service -f
```

The driver uses WeChat iLink `getupdates` long polling, advances the server
cursor, maps allowed users to private identity-isolated sessions, and sends
text replies with the inbound context token. Voice messages are accepted only
when the platform supplies a transcript. An empty allowlist denies every
sender; QR login, media, reactions, typing, and interactive commands are
explicitly not implemented yet and are not downgraded into fake text.

AMQP is also an isolated external driver. It keeps `lapin`, broker credentials,
exchange/queue declarations, acknowledgements, and reconnect policy out of the
core channel host. Configure the runtime-facing socket separately from the
owner-only broker environment:

```dotenv
# /etc/cortexfs/channels/amqp-driver.env, mode 0600
CORTEXFS_AGENT=coder
CORTEXFS_AGENT_SOCKET=/ctx/agent/coder.sock
CORTEXFS_CHANNEL_SESSION_PREFIX=amqp
```

The broker file contains only AMQP settings and secrets:

```dotenv
# /etc/cortexfs/channels/amqp.env, mode 0600
CORTEXFS_AMQP_URL=amqps://user:password@broker/vhost
CORTEXFS_AMQP_EXCHANGE=agent.events
CORTEXFS_AMQP_QUEUE=cortexfs-coder
CORTEXFS_AMQP_ROUTING_KEYS=agent.coder
CORTEXFS_AMQP_PREFETCH=4
CORTEXFS_AMQP_DURABLE_ACK=true
```

Enable the canonical driver socket and AMQP process together:

```bash
sudo systemctl enable --now cortexfs-channel-amqp.service
sudo journalctl -u cortexfs-channel-amqp.service -f
```

Each delivery becomes a provider-neutral text event. The driver publishes a
reply only after the runtime returns successfully, then acknowledges the
delivery. A failed first delivery is requeued once when durable acknowledgements
are enabled; a redelivery is rejected to avoid an unbounded broker loop.
AMQP does not invent attachment, typing, reaction, or thread semantics: those
remain explicit capability work in the channel ABI.

MQTT is an isolated event-source driver. This follows ZeroClaw's current MQTT
role: it is a broker-backed event fan-in, not a chat-platform bot. A subscribed
topic becomes a generic text `InboundMessage`; JSON payloads may provide
`id`, `sender`, `conversation`, `thread`, `reply_to`, `text`, and
`timestamp_ms`. The source topic is retained as ordinary message metadata, so
the core ABI never gains MQTT fields. Replies publish to the inbound topic or
to the configured outbound topic:

```dotenv
# /etc/cortexfs/channels/mqtt.env, mode 0600
CORTEXFS_MQTT_BROKER_URL=mqtts://broker.example.org:8883
CORTEXFS_MQTT_TOPICS=agents/coder,events/agent
CORTEXFS_MQTT_OUTBOUND_TOPIC=agents/replies
CORTEXFS_MQTT_CLIENT_ID=cortexfs-coder
CORTEXFS_MQTT_USERNAME=read-from-a-secret-store
CORTEXFS_MQTT_PASSWORD=read-from-a-secret-store
CORTEXFS_MQTT_QOS=1
CORTEXFS_MQTT_KEEP_ALIVE_SECONDS=30
CORTEXFS_CHANNEL_SOCKET=/run/cortexfs/channel/mqtt.sock
```

Enable the canonical driver socket and MQTT process together:

```bash
sudo systemctl enable --now cortexfs-channel-mqtt.service
sudo journalctl -u cortexfs-channel-mqtt.service -f
```

MQTT attachments, reactions, typing, and interactive commands are rejected
unless a future generic capability is negotiated. Credentials remain in the
owner-only environment file and never cross the channel socket or enter `/ctx`.

Voice calls use the packaged `cortexfs-channel-voice` process. It covers the
ZeroClaw `voice_call` providers Twilio, Telnyx, and Plivo, plus the Telnyx
ClawdTalk Call Control flow. The driver exposes `audio` and `webhook`
capabilities, maps an E.164 caller to a stable call conversation, and keeps
provider credentials outside the runtime. The external socket paths are
`/run/cortexfs/channel/voice_call.sock` and
`/run/cortexfs/channel/clawdtalk.sock`; no per-user TCP port is allocated.

```dotenv
# /etc/cortexfs/channels/voice.env, mode 0600
CORTEXFS_VOICE_CHANNEL=voice_call
CORTEXFS_VOICE_PROVIDER=telnyx
CORTEXFS_VOICE_AUTH_TOKEN=read-from-a-secret-store
CORTEXFS_VOICE_ACCOUNT_ID=connection-id
CORTEXFS_VOICE_FROM_NUMBER=+10000000000
CORTEXFS_VOICE_ALLOWED_DESTINATIONS=+10000000001
CORTEXFS_VOICE_WEBHOOK_BIND=127.0.0.1:8789
CORTEXFS_VOICE_WEBHOOK_BASE_URL=https://voice.example.invalid
CORTEXFS_CHANNEL_SOCKET=/run/cortexfs/channel/voice_call.sock
```

Use `cortexfs-channel-clawdtalk.service` with
`CORTEXFS_VOICE_CHANNEL=clawdtalk` and the `clawdtalk.sock` path. An empty
destination allowlist denies both outbound calls and inbound caller
identities. Runtime replies become provider text-to-speech actions;
`voice_action=hangup` ends an active call.

Twitter/X uses the API v2 mentions timeline. The host confirms the bot user
through `users/me`, keeps a monotonic `since_id` cursor, filters external
identities through an explicit allowlist, and sends replies as tweet threads.
Messages longer than 280 characters are split into a reply chain; direct
messages can be selected by setting the generic `twitter.dm_recipient`
metadata in an external adapter. An empty allowlist denies all senders:

```bash
export CORTEXFS_TWITTER_BEARER_TOKEN='read-from-a-secret-store'
export CORTEXFS_TWITTER_ALLOWED_USERS='alice,123456789'
cortexfs-channel twitter
```

For a packaged deployment, put the values in
`/etc/cortexfs/channels/twitter.env` with mode `0600`, then run
`sudo systemctl enable --now cortexfs-channel-twitter.service`. The API base
defaults to `https://api.x.com/2`; bearer tokens stay in process memory and
are redacted from configuration diagnostics. Media upload, reactions, and
typing are not guessed as text capabilities and remain explicit future work.

Signal uses a local `signal-cli` process boundary. The host keeps Signal's
protocol state outside the message ABI and reconnects the receive process:

```bash
export CORTEXFS_SIGNAL_ACCOUNT='+15551234567'
cortexfs-channel signal
```

Slack Socket Mode is available as a process-isolated full-duplex driver. It
uses the Slack app-level token only to open Socket Mode and the bot token for
message/effect APIs; neither token enters the channel socket payload:

```bash
sudo install -m 600 /dev/null /etc/cortexfs/channels/slack.env
sudoedit /etc/cortexfs/channels/slack.env
sudo systemctl enable --now cortexfs-channel-slack.service
```

The environment file must contain `CORTEXFS_SLACK_APP_TOKEN` and
`CORTEXFS_SLACK_BOT_TOKEN`. Optional values are
`CORTEXFS_SLACK_API_BASE`, `CORTEXFS_SLACK_RECONNECT_SECONDS`, and
`CORTEXFS_SLACK_REPLY_TIMEOUT_SECONDS`. The driver acknowledges Socket Mode
envelopes immediately, maps message threads through the generic route, and
supports proactive sends plus reaction, edit, and delete effects over the
same persistent Unix socket.

Slack Events API, Feishu/Lark, LINE, Microsoft Teams, and Nextcloud Talk retain the explicit webhook
ingress mode:

```bash
export CORTEXFS_CHANNEL_PLATFORM=slack
export CORTEXFS_CHANNEL_OUTBOUND_URL='https://slack.com/api/chat.postMessage'
export CORTEXFS_CHANNEL_BIND=127.0.0.1:8765
cortexfs-channel webhook
```

For LINE, set `CORTEXFS_CHANNEL_PLATFORM=line` and use a URL template such as
`https://api.line.me/{path}`. The channel access token is sent as a bearer
token. Set `CORTEXFS_CHANNEL_VERIFY_TOKEN` to the LINE channel secret to
validate `X-Line-Signature`; the short-lived inbound `replyToken` selects the
Reply API, while later notifications use the Push API.

Telegram and WhatsApp accept one remotely retrievable attachment URL per
outbound message. Telegram selects `sendPhoto`, `sendAudio`, `sendVideo`, or
`sendDocument` from the MIME type; WhatsApp selects the corresponding Graph
media type. Multiple attachments still fail closed instead of silently
dropping files.

For Teams, set `CORTEXFS_CHANNEL_PLATFORM=teams` and use a Bot Framework
service root such as `https://smba.trafficmanager.net/teams/{path}`.
`CORTEXFS_CHANNEL_TOKEN` is sent as the bearer token. The adapter maps Bot
Framework `message` activities to the generic conversation and reply fields;
URL-backed activity attachments use the generic attachment list and are
emitted as Bot Framework activity attachments.

For Nextcloud Talk, set `CORTEXFS_CHANNEL_PLATFORM=nextcloud-talk` and use a
URL template such as `https://talk.example/{path}`. The codec accepts Activity
Streams `create` events and the legacy `message` shape. Set both
`CORTEXFS_CHANNEL_TOKEN` and `CORTEXFS_CHANNEL_VERIFY_TOKEN` to the Talk bot
secret: outbound requests receive the documented HMAC headers, and inbound
requests are checked against `random + raw_body` when the verify secret is
present.

For Slack, use `https://slack.com/api/chat.postMessage` as the outbound URL and
set `CORTEXFS_CHANNEL_TOKEN` to a bot token. For Feishu/Lark, use the tenant
`im/v1/messages` endpoint and set the bearer token as required by the tenant.
`{path}` in `CORTEXFS_CHANNEL_OUTBOUND_URL` is replaced with the codec's
relative API path when a deployment wants one URL template for a codec.

Keep non-Discord credentials in the service manager's environment or secret
store. Do not put tokens in `/ctx`, a repository, command-line arguments, or
durable session metadata.

## Multi-turn and agent capability

The bridge derives one stable session from channel, conversation, thread, and
(for the built-in multi-user host) the external sender identity. It submits text
to the existing `agent/<name>.sock` with `scope=private`, so
the agent retains history, context snapshots, tool calls, approvals, child
agent handoffs, cancellation, and provider routing exactly as a local `ctx
agent send` does.

Discord and Telegram foreground hosts immediately acknowledge an inbound
message, create a bounded “thinking” placeholder, and coalesce streamed delta
events into platform edits. They remove the acknowledgement on completion and
show a failure reaction plus a visible error when the agent or provider fails.
If a platform operation is unavailable, the host falls back to one final
message; progress effects never become durable conversation facts.

The same inbound message produces the same idempotency key. The socket runtime
therefore handles retries through its existing replay rules and remains the
single writer of durable session facts.

The WebSocket frontend is full-duplex: after its initial `input`, it can send
`status`, `resume`, `cancel`, or another `input` while the first run is still
streaming, and it can answer a runtime `command` on the same connection. The
host correlates these requests and uses separate existing agent-socket streams
internally; the browser and terminal therefore share one interaction ABI.

## Coverage boundary

The current built-in host covers Telegram long polling, Bluesky AT Protocol
notification polling, Discord Gateway,
DingTalk Stream Mode, Matrix Client-Server sync, Slack Socket Mode and
Events/webhook,
Feishu/Lark webhook, LINE Messaging API webhook, Microsoft Teams Bot
Framework webhook, Nextcloud Talk webhook, WhatsApp Business Cloud webhook, Gmail Push, IMAP/SMTP
email, Signal via `signal-cli`, IRC, Twitch TLS IRC, Reddit OAuth inbox,
Mattermost WebSocket/REST, QQ Bot Gateway/REST, and the send-only WeCom Bot
Webhook, WeChat iLink long polling, Mochat HTTP polling, Linq Partner webhook, and Notion database
polling. The
Twitter/X API v2 mentions host is also native and uses the same poll/cursor
boundary. The
independent crate exposes the
provider-neutral message, lifecycle, capability, effect, and socket ABI; its
stateless codecs cover all of those native payload families, including
Twitter tweet/reply and direct-message request shapes.

Nostr is supplied by the packaged `cortexfs-channel-nostr` process-isolated
driver. It is intentionally not counted as a built-in host: the generic
`cortexfs-channels` crate remains free of relay, key, and NIP-specific types.

AMQP is supplied by the packaged `cortexfs-channel-amqp` process-isolated
driver. The generic crate remains free of `lapin`, broker credentials, and
RabbitMQ-specific types; its socket ABI is the only runtime boundary.

MQTT is supplied by the packaged `cortexfs-channel-mqtt` process-isolated
driver. It is classified as an external event source rather than a native IM
adapter; the generic crate remains free of `rumqttc`, broker credentials, and
topic-specific types.

WeCom AI Bot WebSocket is supplied by the packaged
`cortexfs-channel-wecom-ws` process-isolated driver. The generic crate remains
free of the WeCom subscription protocol and secrets; it only sees the common
socket and message ABI.

WeChat iLink is supplied by the packaged `cortexfs-channel-wechat`
process-isolated driver. The generic crate remains free of the iLink HTTP
protocol and bot token; it only sees the common socket and message ABI.

Slack Socket Mode is supplied by the packaged
`cortexfs-channel-slack` process-isolated driver. It keeps Slack's two token
classes and WebSocket envelope acknowledgements outside the generic ABI while
supporting runtime-initiated delivery and live message effects.

Voice and ClawdTalk are supplied by the packaged `cortexfs-channel-voice`
process. Their provider APIs and webhook parsing stay outside the generic
channel crate; the runtime receives only audio-capable generic messages and
delivery receipts.

ZeroClaw's documented channel set includes voice wake, macOS-only iMessage,
and a CLI frontend. CortexFS records `voice_wake` as an explicit external
audio capability; the wake-word engine and microphone permissions remain a
separate host integration. CortexFS's CLI channel is already provided by `ctx agent chat` /
`ctxchat`, which uses the same interaction ABI rather than a second channel
implementation. iMessage remains an OS-specific external-driver integration;
on Linux it can use the generic `cortexfs-channel driver` socket without
adding Apple APIs to the core. The
current Email, Gmail, Signal, and IRC implementations also have the explicit
limitations above; attachment, signature, E2EE, and platform-specific
streaming behavior must be added as capabilities rather than silently treated
as text. The generic `driver` command remains the process-isolated extension
point for third-party adapters during migration.

The public `cortexfs-channels::CHANNEL_CATALOG` records the complete upstream
channel-family inventory. Entries marked `native=false` are process-isolated or
OS-specific: packaged drivers such as Nostr, AMQP, WeChat, WeCom WebSocket,
voice, and ClawdTalk use the same `cortexfs.channel.socket/v1` boundary, while
macOS-only or local event-source integrations remain explicit external-host
work.

### Channel-local tools

Every catalog entry exposes common tools plus named platform operations below
its own tool directory. Examples are `telegram.send_photo`,
`discord.send_component`, `slack.send_blocks`, `email.search`,
`gmail.fetch_message`, `matrix.invite_user`, `git.pull_request`, and
`voice_call.start_call`. The full inventory is generated from
`ChannelSpec::platform_tool_names()` so filesystem discovery, policy, and the
ZeroClaw comparison cannot drift apart. These executables send a generic
`ChannelCommand::Invoke`; the adapter is responsible for native API behavior,
authentication, rate limits, and unsupported-operation errors.

## Extending from another agent application

Add the public crate and implement `ChannelAdapter` for a platform transport:

```toml
cortexfs-channels = "0.1"
```

The adapter can use any HTTP/WebSocket/runtime stack. It reports platform
features through `ChannelCapabilities`, returns `DeliveryReceipt` values, and
registers in `ChannelRegistry`. If a platform has webhook JSON, its stateless
codec can implement `ChannelCodec` without depending on CortexFS. The agent
application remains responsible for binding an inbound conversation to its
own durable session.

The normative contract is in
[spec/channel-abi.md](spec/channel-abi.md); the crate's Rust API is the source
for exact types and trait signatures.
