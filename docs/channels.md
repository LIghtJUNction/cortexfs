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

After installing the normal CortexFS package, start an agent and point the
channel host at its visible runtime socket:

```bash
ctx agent start coder --session default
export CORTEXFS_AGENT_SOCKET=/run/cortexfs/agent/coder.sock
export CORTEXFS_AGENT=coder
```

Telegram uses long polling:

```bash
export CORTEXFS_TELEGRAM_TOKEN='...'
cortexfs-channel telegram
```

Discord, Slack and Feishu/Lark use webhook ingress plus their normal HTTP send
API:

```bash
export CORTEXFS_CHANNEL_PLATFORM=discord
export CORTEXFS_CHANNEL_OUTBOUND_URL='https://discord.com/api/webhooks/<id>/<token>'
export CORTEXFS_CHANNEL_BIND=127.0.0.1:8765
cortexfs-channel webhook
```

For Slack, use `https://slack.com/api/chat.postMessage` as the outbound URL and
set `CORTEXFS_CHANNEL_TOKEN` to a bot token. For Feishu/Lark, use the tenant
`im/v1/messages` endpoint and set the bearer token as required by the tenant.
`{path}` in `CORTEXFS_CHANNEL_OUTBOUND_URL` is replaced with the codec's
relative API path when a deployment wants one URL template for a codec.

Keep credentials in the service manager's environment or secret store. Do not
put tokens in `/ctx`, a repository, command-line arguments, or durable session
metadata.

## Multi-turn and agent capability

The bridge derives one stable session from channel, conversation, and thread.
It submits text to the existing `agent/<name>.sock` with `scope=private`, so
the agent retains history, context snapshots, tool calls, approvals, child
agent handoffs, cancellation, and provider routing exactly as a local `ctx
agent send` does. Only the final assistant text is sent back to the platform.

The same inbound message produces the same idempotency key. The socket runtime
therefore handles retries through its existing replay rules and remains the
single writer of durable session facts.

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
