# Channel ABI

This specification defines the public `cortexfs-channels` crate boundary and
the optional CortexFS channel host. It does not add a `/ctx/channel` tree or a
second durable submission mechanism. Channel delivery enters the existing
`agent/<name>.sock` JSONL session ABI.

## Public crate

The released package is [cortexfs-channels on crates.io](https://crates.io/crates/cortexfs-channels).

`cortexfs-channels` is runtime-neutral. It depends on neither CortexFS nor
FUSE, a model provider, an HTTP client, nor a particular async executor. It
exports:

- `ChannelAdapter`: object-safe `listen`, `send`, capability, and health
  methods;
- `ChannelRegistry`: named adapter registration and dispatch;
- `InboundMessage` and `OutboundMessage`: one target model for conversations,
  threads, reply ids, participants, text, attachments, and metadata;
- `ChannelSessionRoute`: deterministic conversation-to-session mapping;
- `ChannelEnvelope`: versioned JSON boundary with ABI value
  `cortexfs.channel/v1`;
- `platform::{telegram, discord, slack, feishu}`: stateless payload codecs.

An adapter owns authentication, rate limiting, reconnect policy, and the
platform transport. The shared layer only owns the semantic contract. A host
may implement a new adapter without changing any agent or filesystem ABI.
The packaged Discord host reads its credentials and routing values from one
owner-only TOML file; it does not write channel state into `/ctx`.

## Message and session semantics

The tuple `(channel, conversation, thread)` identifies a multi-turn context.
The CortexFS bridge maps it to `im-<prefix>-<stable-hash>` and sends the text
through the existing request:

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

## Built-in host

`cortexfs-channel` is an explicit foreground process. It is not a `ctx`
subcommand, does not create a new root namespace, and does not watch files.
Discord is configured with `/etc/cortexfs/channels/discord.toml` and runs over
the Gateway; webhook modes remain available for the other stateless hosts:

The built-in configuration contract uses the public client endpoint
`/ctx/agent/<agent>.sock`, derived by `cortexfs-paths::agent_client_socket`.
Private `/run/cortexfs/agent/<agent>.sock` paths are systemd implementation
details and are rejected by the channel config loader. A host must verify the
selected storage generation and live agent socket before advertising itself as
ready.

```bash
# Discord Gateway
sudo systemctl enable --now cortexfs-channel@discord.service

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

The codec currently covers Telegram text updates, Discord message/webhook
payloads, Slack message events and URL verification, and Feishu/Lark text
events. Discord and Telegram foreground hosts support best-effort reactions,
typing indicators, placeholder messages, and coalesced edits; webhook hosts
remain final-message only until their platform-specific edit contract is
configured. Media and signature/encryption features remain explicit capability
work rather than being guessed as text.
