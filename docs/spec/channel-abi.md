# Channel ABI

This specification defines the public `cortexfs-channels` crate boundary and
the optional CortexFS channel host. Channel state and tools use the explicit
`/ctx/channel` root; delivery enters the existing
`agent/<name>.sock` JSONL session ABI.

## Public crate

The released package is [cortexfs-channels on crates.io](https://crates.io/crates/cortexfs-channels).

`cortexfs-channels` is runtime-neutral. It depends on neither CortexFS nor
FUSE, a model provider, an HTTP client, nor a particular async executor. It
exports:

- `ChannelAdapter`: object-safe connect/start, unified `receive_incoming`,
  compatible `receive`/`listen` streams, send, reconnect, capability, and
  health methods;
- `ChannelRegistry`: named adapter registration and dispatch;
- `InboundMessage` and `OutboundMessage`: one target model for conversations,
  threads, reply ids, participants, text, attachments, and metadata;
- `ChannelIncoming`: an additive receive item that distinguishes a message
  from a provider-neutral `ChannelIncomingEvent`;
- `ChannelSessionRoute`: deterministic conversation-to-session mapping;
- `CHANNEL_CATALOG`: discoverable upstream channel families and their transport
  boundary/native-host status;
- `ChannelEnvelope`: versioned JSON boundary with ABI value
  `cortexfs.channel/v1`;
- `ChannelFrame`: bidirectional JSONL socket boundary with ABI value
  `cortexfs.channel.socket/v1`, correlation ids, lifecycle, health, delivery,
  receipts, live effects, and provider-neutral runtime commands/results;
- `ChannelControlAction`: provider-neutral Tool SDK requests for sending,
  effects, and commands without platform-specific message types;
- `platform::{telegram, bluesky, discord, slack, feishu, lark, dingtalk, line, teams, nextcloud, matrix, whatsapp, gmail, email, signal, irc, twitch, reddit, mattermost, qq, linq, notion}`: stateless payload codecs; each codec exposes the catalog-derived `capabilities()` declaration.
- `cortexfs-channel-nostr`: an optional process-isolated driver for NIP-04/NIP-17 and relay WebSockets; it speaks only `cortexfs.channel.socket/v1` and keeps Nostr key/encryption types outside this ABI crate.
- `cortexfs-channel-amqp`: an optional process-isolated AMQP driver; it speaks only `cortexfs.channel.socket/v1` and keeps `lapin`, broker credentials, and acknowledgement semantics outside this ABI crate.
- `cortexfs-channel-wecom-ws`: an optional process-isolated WeCom AI Bot WebSocket driver; it speaks only `cortexfs.channel.socket/v1` and keeps subscription credentials, heartbeats, and stream frames outside this ABI crate.
- `cortexfs-channel-wechat`: an optional process-isolated WeChat iLink long-polling driver; it speaks only `cortexfs.channel.socket/v1` and keeps bot tokens, cursors, and context tokens outside this ABI crate.
- `cortexfs-channel-slack`: an optional process-isolated Slack Socket Mode driver; it speaks only `cortexfs.channel.socket/v1` and keeps app/bot tokens, WebSocket envelopes, and Slack API effects outside this ABI crate.
- `cortexfs-channel-mqtt`: an optional process-isolated MQTT event-source driver; it speaks only `cortexfs.channel.socket/v1` and keeps broker credentials, topics, reconnect state, and MQTT packet types outside this ABI crate.

An adapter owns authentication, rate limiting, reconnect policy, and the
platform transport. The shared layer only owns the semantic contract. A host
may implement a new adapter without changing any agent or filesystem ABI.
The packaged Discord host reads its credentials and routing values from one
owner-only TOML file; it does not write channel state into `/ctx`.

`ChannelId` is the complete channel-instance key. A value such as
`telegram.primary` has family `telegram` and instance `primary`; the catalog
uses the family for capability discovery, while registries, driver hubs,
delivery targets, and session routes retain the complete value. Therefore two
accounts can run as `telegram.primary` and `telegram.secondary` on separate
driver sockets without a per-user TCP port or a new `/ctx` namespace. The
instance suffix is generic and carries no Telegram- or provider-specific ABI.
Hosts using a stateless codec must call `decode_incoming_for` (or its batch
variant) with the configured complete id; this preserves the alias on both
messages and provider-neutral events before session routing.
The built-in foreground hosts expose the same binding as the optional
`CORTEXFS_CHANNEL_ID` setting. The disk-backed Discord host uses the equivalent
optional `channel = "discord.primary"` TOML value. It is an instance
identifier only; credentials, session data, and channel state remain outside
`/ctx`.

Capabilities are directional where the distinction affects routing: the
legacy `attachments` flag remains the compatibility aggregate, while
`receive_attachments` and `send_attachments` describe the two data paths.
`draft_updates` means that the active host can update one in-progress reply;
`multi_message_streaming` means that it can emit several independent streamed
messages. These flags are advisory and should be checked before emitting an
effect. `ChannelActions` is the corresponding fine-grained declaration for
typing, preview, reaction, edit, delete, mark-read, pin, unpin, and redact
operations; old `Hello` frames may omit it and decode as an empty declaration.
`ChannelCapabilities::choices` and `multi_choice` describe the separate
provider-neutral `RequestChoice` command. Linq URL-backed media parts use the
same attachment URL ABI for receive and send; raw media upload is still
adapter-owned.
`ChannelCapabilities::tool_control` is separate from interactive `commands`:
it means the adapter accepts runtime-to-platform `Invoke` operations emitted by
channel-local tools. An adapter must advertise it during `Hello` before those
operations are forwarded.
After a valid `Hello`, the runtime gates live `Effect` frames against the
declared `ChannelActions` and rejects runtime `Command` frames locally when
`capabilities.commands` is false. This keeps an adapter from silently
discarding progress frames or leaving an Agent waiting for a UI it does not
have. Connections that do not send `Hello` retain the pre-negotiation legacy
behavior for compatibility.
Discord and Slack currently encode URL-backed outbound attachments as native
embeds/blocks. Uploading raw bytes and platform-specific authentication remain
adapter-owned concerns.
Mattermost and Teams likewise preserve URL-backed attachments through their
native post/activity properties; raw file upload remains adapter-owned.
The Mattermost adapter also maps its WebSocket reaction, edit, delete, and
typing events to `ChannelIncomingEvent` and its REST reaction/edit/delete
operations to `ChannelEffect`.

Stateless webhook codecs must call `decode_incoming()` rather than only
`decode()`. The event-first ordering lets a reaction, typing signal, edit, or
delete bypass message parsing and enter the same bridge as an inbound message;
the bridge derives a stable event request id and applies identity-isolated
session routing when the route enables it.

## Channel filesystem and Tool SDK

Each channel instance owns its generic tools and read-only global state:

```text
/ctx/channel/<name>/tool/<tool>
/ctx/channel/<name>.d/{id,driver,cap,status,health}
/ctx/channel/<name>.d/adapter          optional; catalog family or custom object name
/ctx/channel/<name>.d/adapter.d/<name> optional custom socket driver executable
/ctx/home/<uid>/channel/<name>/tool/<tool>
/ctx/home/<uid>/channel/<name>.d/*
```

`driver` keeps the ABI string such as `cortexfs.channel.socket/v1`. The optional
`adapter` control selects a catalog family (`telegram`, `discord`, …) or a custom
object name. A custom name resolves `adapter.d/<name>` when that executable is
present; otherwise the host keeps the built-in catalog driver for the family.
The Channel SDK exposes `DriverLaunchConfig::from_env()` so custom adapters read
`CORTEXFS_CHANNEL_ID`, socket path, request prefix, and reply timeout without
re-parsing host conventions.

There is intentionally no `/ctx/channel/tool`. Common operations keep generic
names (`channel.send`, `channel.reply`, `channel.react`, `channel.choice`,
`channel.draft_*`, `channel.gate_*`, `channel.room_*`, and so on) but are
installed inside every channel namespace. Platform operations use names such as
`telegram.send_photo`, `discord.send_embed`, `email.search`,
`gmail.register_watch`, `matrix.create_room`, or `git.forge_request`; the
catalog is the source of truth for the complete list. The `<channel>.invoke`
name remains an open extension point. All platform operations carry only the
provider-neutral `Invoke { name, payload }` shape; platform request/response
types stay in the adapter.
User channel tools precede global channel tools for that user. A collision with
the Agent's existing tool path fails closed instead of silently shadowing it.

For a channel-backed run, Runtime constructs the effective `CTX_PATH` and
injects `CTX_CHANNEL_ID`, `CTX_CHANNEL_SESSION`, `CTX_CHANNEL_CAPS`, and
`CTX_CHANNEL_SOCKET` into the Agent child. Identity, thread, attachments, and
other structured origin data remain in the stdin JSONL envelope. Secrets never
enter these values.

`ControlHello` creates a controller connection without replacing the registered
adapter. `ControlRequest` carries a `ChannelControlAction`; the driver checks
the adapter's negotiated capabilities/actions, forwards `Outbound`, `Effect`,
or `Command` to the adapter, and returns a correlated `ControlResponse`.

## Message and session semantics

The public route default treats `(channel, conversation, thread)` as one
multi-turn context. A host serving multiple external users can opt into the
same generic route's identity-isolation mode; then the sender identity is
included in the stable key without introducing a Telegram- or Discord-specific
field. The CortexFS built-in host enables that mode for private channel
sessions. It maps the selected key to `im-<prefix>-<stable-hash>` and sends the
text through the existing request:

```json
{"op":"send","id":"im-<prefix>-<message-hash>","session":"im-<prefix>-<conversation-hash>","scope":"private","input":"hello"}
```

The existing agent socket file remains the source of truth for idempotency, durable
`messages.jsonl`/`events.jsonl`, tool authorization, model execution, and
assistant event framing. Re-delivering the same platform message therefore
replays the existing session result instead of executing the agent twice.

The bridge may consume the same stream incrementally. Built-in interactive
hosts use `start`, `delta`, `message`, `error`, and `done` frames to provide a
transport-local acknowledgement, typing indicator, bounded placeholder edit,
and final message. These progress effects are not written to session history;
the socket remains the source of truth. Errors and tool events are not exposed
as assistant text unless the host chooses a user-facing failure notice.

`ChannelAdapter::receive_incoming()` is the canonical receive operation. It
returns one stream whose item is either `ChannelIncoming::Message` or
`ChannelIncoming::Event`; an adapter that only implements the legacy
`listen()` message stream is lifted automatically. `receive_events()` remains
available for adapters that need a separate native event implementation and
for source compatibility, but hosts should route the unified stream.

The socket driver boundary is deliberately separate from the frontend/runtime
interaction ABI. A driver sends `Inbound` frames to the channel runtime; the
runtime sends `Deliver` or `Effect` frames back. `InboundEvent` carries
provider-neutral reactions, typing, message edits/deletes, and read receipts;
the runtime forwards its structured value to the executable-agent envelope
without adding provider fields to the message ABI. `Outbound` is the independent
runtime-to-driver delivery used when no preceding inbound event exists; the
driver acknowledges it with a correlated `Receipt`. `Deliver`, `Outbound`,
`Effect`, `Event`, `Health`, `HealthRequest`, `HealthResponse`, `Receipt`,
`Command`, and `CommandResult` are independent stream frames, not a lock-step
request/response pair: either side
may emit its frame after the connection has been established. A runtime command carries only
the provider-neutral input/approval/notify/invoke shape; platform adapters
decide how to present it and return a `CommandResult`. This keeps a channel
driver able to answer an Agent while the Agent is still running, without
putting Telegram, Discord, or Email types into the ABI. `Typing` and `Preview`
effects are live, bounded progress signals: they are not durable messages, and
a driver may ignore them when its platform has no equivalent. The runtime
writes these effects as they arrive and retains only the bounded preview text,
so a long model stream does not accumulate every delta in memory. `Start` and
`Stop` describe lifecycle without binding an adapter to an Agent or a
filesystem path. `request_id`/`event_id` values correlate retries and receipts;
they do not replace the durable session idempotency rules.

`Health` is an unsolicited compatibility report. `HealthRequest` and
`HealthResponse` form the correlated health probe for a live socket; either
peer may issue the request, and the peer must return only sanitized
`ChannelHealth` data. This makes health checks usable without a second port or
platform-specific control path. A one-shot driver that may receive runtime
traffic during the probe uses `health_with_handlers` so `Command`, `Outbound`,
and `Effect` frames are handled before the correlated response; the legacy
`health()` helper fails closed instead of silently dropping an unsolicited
outbound delivery.

A `Command` also carries an optional `MessageTarget`. It is absent in older
peers for compatibility, but new drivers should use it to present an input or
approval request in the correct conversation. `ChannelCapabilities::commands`
is the advertised opt-in for this round trip. The process-isolated Slack
Socket Mode driver implements notify, input, and approval commands through
messages and interactive action callbacks; generic invoke remains rejected.
The built-in process-isolated `driver` host accepts at most four in-flight
inbound runs per Unix-socket connection. This bound keeps concurrent IM
conversations independent while preventing an external adapter from creating
unbounded runtime threads on small hosts; another channel instance may use a
separate connection.

Runtime integrations that need a message without a preceding inbound event use
the `DriverHub` attached to the driver host. `send_and_wait` writes an
`Outbound` frame to the persistent driver connection and waits for the
driver-side `Receipt` at the adapter boundary; `send` is the fire-and-forget
variant. The hub is keyed by the canonical channel id, not by a TCP port or a
new `/ctx` object. A driver that only implements one-shot
`Inbound`/`Deliver` exchange remains compatible, but cannot receive proactive
outbound traffic until it keeps the socket open and consumes frames. New
process-isolated adapters should use `ChannelDriverSession`, whose reader
thread keeps `Outbound`, `Event`, and correlated delivery frames available
while the platform transport is idle; `ChannelDriverClient::next_frame()` is
the lower-level manual option. A blocking one-shot adapter that also needs to
answer runtime input or approval commands can use
`ChannelDriverClient::deliver_with_command_handler`; the legacy `deliver`
methods still reject commands instead of auto-approving them.
One-shot adapters that need to render runtime progress or other live effects
can use `deliver_with_all_handlers` (or its incoming-event sibling), which
handles `Effect`, `Command`, and proactive `Outbound` frames before the final
`Deliver`. The older handler methods remain compatible and deliberately ignore
effects when the adapter has no live-effect sink.

Runtime commands use the same interaction ABI on the agent socket. The
terminal host can answer `approval_request` with `CommandResult` and the
runtime verifies the original request id, run, and tool-call id. The current
one-shot web POST and native channel hosts return a bounded rejection when
their frontend has no command reply path; they never auto-approve a tool and
never leave the runtime waiting for an unavailable reply. An external channel
driver using the socket ABI can opt into the full path by forwarding
`Command` frames to its platform and returning the correlated `CommandResult`.
Slack Socket Mode is one such persistent driver: it keeps pending input and
approval correlations in memory for the live connection and sends the
result frame only after the user response arrives.

The built-in `cortexfs-channel web` host exposes the same interaction frames
over both `POST /v1/interaction` and a WebSocket upgrade on the same
configurable path. POST accepts one `cortexfs.interaction/v1` request and
returns the runtime event stream as newline-delimited JSON. WebSocket clients
send the initial request as one interaction frame, receive the same event
frames, and can answer runtime commands on that connection after validating
the request, session, and command ids. While that run is active, the client
may also send `input`, `resume`, `status`, or `cancel`; the host submits each
request through a separate existing agent-socket stream and merges its events
back into the same WebSocket. Thus the connection is full-duplex without
changing the agent socket ABI or introducing a web-specific command type. The
default bind address is loopback; a non-loopback bind requires
`CORTEXFS_WEB_TOKEN`, which is checked as an HTTP Bearer token.

## Built-in host

`cortexfs-channel` is an explicit foreground process. It is not a `ctx`
subcommand, does not create a new root namespace, and does not watch files.
`cortexfs-channel list`, `show FAMILY`, and `preset FAMILY` are host-side
discovery helpers: they print the catalog, required secrets, and an env
template to stdout. They do not start a transport or write `/ctx`.
Inbound `/help`, `/models`, `/model`, and `/new` are answered by the existing
channel bridge before the agent socket; `/new` only rotates the in-memory
session generation for that host process.
The CLI frontend is intentionally `ctx agent chat` / `ctxchat`; it shares the
interaction ABI and does not introduce a second CLI channel protocol.
Use `cortexfs-channel web` for a browser/API frontend and `webhook` for a
platform-native callback; both enter the same agent socket interaction ABI.
Discord is configured with `/etc/cortexfs/channels/discord.toml` and runs over
the Gateway. Telegram uses long polling, DingTalk uses Stream Mode, and Matrix
uses the Client-Server `/sync` loop; webhook modes remain available for the
other stateless hosts:

The built-in configuration contract uses the public client endpoint
`/ctx/agent/<agent>.sock`, derived by `cortexfs-paths::agent_client_socket`.
Private `/run/cortexfs/agent/<agent>.sock` paths are systemd implementation
details and are rejected by the channel config loader. A host must verify the
selected storage generation and live agent socket before advertising itself as
ready.

```bash
# Discord Gateway
sudo systemctl enable --now cortexfs-channel@discord.service

# DingTalk Stream Mode
export CORTEXFS_DINGTALK_CLIENT_ID='read-from-a-secret-store'
export CORTEXFS_DINGTALK_CLIENT_SECRET='read-from-a-secret-store'
cortexfs-channel dingtalk

# Matrix Client-Server sync
export CORTEXFS_MATRIX_HOMESERVER='https://matrix.example.org'
export CORTEXFS_MATRIX_ACCESS_TOKEN='read-from-a-secret-store'
cortexfs-channel matrix

# Slack Events API or Feishu/Lark webhook
export CORTEXFS_CHANNEL_PLATFORM=slack
export CORTEXFS_CHANNEL_BIND=127.0.0.1:8765
export CORTEXFS_CHANNEL_OUTBOUND_URL=https://slack.com/api/chat.postMessage
export CORTEXFS_CHANNEL_TOKEN='read-from-a-secret-store'
cortexfs-channel webhook
```

For webhook mode, configure the platform callback URL to
`http://<host>:8765/webhook` (or the configured bind/path) and put a
signature-verifying reverse proxy in front of the listener. The small built-in
listener validates framing and bounds the body, but platform signature
verification is deployment policy and must not be silently bypassed.

The codec currently covers Telegram text and event updates, Bluesky notification and
post records, Discord message/webhook
payloads, Slack message events and URL verification, Feishu/Lark text events,
DingTalk Stream callback frames, LINE Messaging API events and reply/push
requests, Microsoft Bot Framework Teams activities, Nextcloud Talk Activity
Streams and legacy message webhooks, Matrix `m.room.message`, reaction,
replacement, and redaction events, WhatsApp
Business Cloud text webhooks/Graph API sends, Gmail Pub/Sub and message
resources, RFC 5322 email, Signal CLI envelopes, IRC/Twitch `PRIVMSG` lines,
Reddit OAuth inbox/comment records, Twitter/X API v2 mentions and reply records,
WeCom Bot Webhook text sends, Linq Partner webhook messages, Mattermost `posted` WebSocket events/REST posts,
Mochat receive/send records, Notion database pages, and QQ Bot Gateway guild,
group, and C2C events/REST posts.
WhatsApp is selected through the generic webhook host with
`CORTEXFS_CHANNEL_PLATFORM=whatsapp`; the outbound URL must include the Graph
API phone-number endpoint and the bearer token is passed by the host.
Discord and Telegram foreground hosts support best-effort reactions, typing
indicators, placeholder messages, and coalesced edits; DingTalk replies use
the per-message session webhook. Matrix foreground sync consumes the same
message/event stream; Matrix replies use `m.in_reply_to` or thread relations.
Webhook hosts continue to deliver a final message; codecs may additionally
emit best-effort live effects such as a typing indicator when the platform has
an equivalent. Placeholder edits, streamed previews, media and
signature/encryption features remain explicit capability work rather than
being guessed as text.
