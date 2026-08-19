# cortexfs-channels

Runtime-neutral multi-channel messaging abstractions for agent software.

The crate defines one message model, one adapter lifecycle (connect, start,
receive/listen, send, reconnect, and stop), registry-based routing, delivery
receipts, health reporting, fine-grained capabilities, and a versioned JSON
envelope.
It does not depend on CortexFS, FUSE, a model provider, an HTTP runtime, or a
particular IM platform. Platform codecs and host runtimes can be layered on
top without changing the agent-facing ABI.

```toml
cortexfs-channels = "0.1"
```

See the crate documentation for the adapter contract and the `platform`
module for reusable webhook codecs.

Built-in stateless codecs include Telegram, Bluesky AT Protocol, Discord, Slack, Feishu/Lark,
DingTalk Stream, LINE Messaging API, Microsoft Teams Bot Framework, Nextcloud
Talk Bot Activity Streams, Matrix Client-Server, WhatsApp Business Cloud,
Gmail Push, RFC 5322 email, Signal CLI envelopes, IRC/Twitch `PRIVMSG`,
Reddit OAuth inbox/comment payloads, Mattermost WebSocket post payloads, and
QQ Bot Gateway guild/group/C2C events and REST posts, and Twitter/X API v2
mentions and reply payloads, WeCom Bot Webhook text sends, and Linq Partner
webhook messages. The
Mochat receive/send API and Notion database pages are also represented by
stateless codecs; their hosts
owns the cursor, bearer token, retry loop, and sender allowlist. The
crate does not open network connections;
the host or adapter owns polling, WebSocket lifecycle, credentials, retries,
and platform-specific delivery details.

The optional `cortexfs.channel.socket/v1` driver boundary also carries
provider-neutral `Command` and `CommandResult` frames. A command includes an
optional generic `MessageTarget`, so a driver knows which conversation should
present it while older peers that omit the field remain decodable. A driver
can therefore present an Agent approval or input request on its platform and
return the correlated result while the Agent run is still active; no
platform-specific message type enters this crate. `ChannelCapabilities::commands`
advertises whether that round trip is implemented.

The runtime side can use its `DriverHub` to emit a provider-neutral `Outbound`
frame when no inbound event triggered the send. `send()` is fire-and-forget;
`send_and_wait()` also correlates the driver receipt with a bounded timeout. A
persistent driver should use `ChannelDriverSession::recv()` so unsolicited
runtime frames remain available while the platform is idle, and acknowledge
platform delivery with `ChannelDriverSession::send_receipt()`; the older
`ChannelDriverClient::next_frame()` helper remains available for a manually
managed persistent connection. One-shot drivers continue to use `Deliver`
unchanged.

Process-isolated adapters may reuse `ChannelDriverClient` for the common
`Hello`/`Start`/`Inbound`/`InboundEvent`/`Deliver` exchange. It is a small blocking Unix
socket helper for external drivers; it does not start an Agent, allocate a
port, or persist channel state. Tool SDK executables use a separate
`ControlHello`/`ControlRequest` connection for generic sends, effects, and
commands; it never replaces the adapter connection. A persistent driver can use
`ChannelDriverSession` to receive runtime-initiated `Outbound` deliveries even
while idle and return the provider-neutral acknowledgement. The optional Nostr,
AMQP, MQTT, Slack Socket Mode, WeChat iLink, WeCom WebSocket, and voice/ClawdTalk drivers use this
boundary while keeping relay encryption,
broker protocols, long-polling credentials, and subscription secrets out of
the reusable ABI crate.

`InboundEvent` carries reactions, typing, edits, deletes, and read receipts
through the same provider-neutral socket. Its `ChannelEventContext` retains
the canonical target, optional participant, timestamp, and metadata; platform
payload types stay inside the adapter.

For a blocking one-shot adapter, `ChannelDriverClient::deliver_with_handlers`
and its incoming-event sibling keep the delivery exchange synchronous while
allowing the adapter to present `Command` frames, handle proactive `Outbound`
frames, and return correlated `CommandResult`/`Receipt` frames. The narrower
`deliver_with_command_handler` helper retains the safe rejection behavior for
proactive outbound traffic when no handler exists. `health()` correlates a
`HealthRequest` with a sanitized `HealthResponse`; `ChannelDriverSession` can
send the same probe without taking control of its receive loop. One-shot
adapters that must preserve runtime traffic during that probe can use
`health_with_handlers` for `Command`, `Outbound`, and `Effect` frames; the
legacy helper fails closed if an unsolicited outbound delivery has no handler.
Adapters that also need live typing, preview, reaction, edit, delete, or
mark-read frames can use the `*_with_all_handlers` variants; the older handler
methods remain source-compatible and intentionally ignore effects.

`ChannelAdapter::receive_incoming()` is the canonical unified stream for
`ChannelIncoming::Message` and `ChannelIncoming::Event`. Existing adapters
that only implement `listen()` are lifted into that stream automatically;
`receive_events()` remains available for adapters with a native event stream
and for source compatibility. The stateless codec API mirrors this with
`decode_incoming()`, which checks provider-neutral events before messages so
event-shaped payloads cannot be misclassified as malformed messages. Telegram,
Discord, and Slack codecs normalize reactions, typing, edits, deletes, and
mentions; Matrix additionally normalizes reactions, edits, and redactions
without exposing platform types. Webhook hosts consume the same
`ChannelIncoming` value and route both variants through the common bridge.

`CHANNEL_CATALOG` is the discovery list for the current ZeroClaw channel
families. Its `native` flag distinguishes built-in CortexFS hosts from
platforms that should currently be supplied through the same isolated driver
socket; an unconfigured third-party transport is never reported as native.
`ChannelId` is an instance key, not merely a platform enum: `telegram.primary`
and `telegram.secondary` are two independent channel instances in the same
`telegram` family. Use `ChannelId::family()` for catalog/capability lookup and
retain the complete id for registry, driver, delivery, and session routing.
This lets multiple accounts share one runtime without assigning one TCP port
per account; each instance may instead use
its own configured driver socket.
Native hosts may set `CORTEXFS_CHANNEL_ID=telegram.primary` (or the equivalent
family/instance value) to preserve that complete id when a stateless codec
decodes inbound messages and events.
Stateless codecs expose `decode_incoming_for` and
`decode_many_incoming_for` so a host can rebind decoded messages/events to that
complete instance id before routing; using the base codec methods remains
backward-compatible for single-instance hosts.
Capability declarations keep the old aggregate `attachments` flag while also
exposing `receive_attachments`, `send_attachments`, `draft_updates`, and
`multi_message_streaming`. `ChannelActions` separately advertises typing,
preview, reaction, edit, delete, mark-read, pin, unpin, and redact effects; its
field in `Hello` is optional for older socket peers. `ChannelCommand::RequestChoice`
provides a correlated single- or multi-choice prompt without requiring a
platform-specific message type; `ChannelCapabilities::choices` and
`multi_choice` describe whether the adapter can present those prompts. Linq
supports URL-backed text and media parts in both directions; raw uploads
remain adapter-owned.
The runtime uses the `Hello` declaration for live-frame gating: an advertised
unsupported effect is not sent, and a driver that does not advertise
`commands` receives an immediate correlated rejection instead of a command
that can wait forever. Peers that predate capability negotiation retain the
legacy effect path until their `Hello` is observed.
The Discord, Slack, Mattermost, and Teams codecs encode URL-backed outbound
attachments through native embeds/blocks/post/activity properties; uploading
raw bytes remains the responsibility of a platform adapter.
Mattermost also normalizes reaction, edit, delete, and typing WebSocket events
and encodes reaction/edit/delete effects through its REST API.
The process-isolated Slack Socket Mode driver advertises `commands` and maps
notify, input, and approval commands to Slack messages and interactive action
callbacks; generic invoke commands remain explicitly rejected.
