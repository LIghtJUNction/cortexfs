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
agent_socket = "/ctx/agent/coder.sock"
agent = "coder"
session_prefix = "discord"
```

Start the low-memory synchronous Gateway adapter after enabling the Discord
`MESSAGE_CONTENT` privileged intent in the Discord Developer Portal:

```bash
sudo systemctl enable --now cortexfs-channel@discord.service
sudo journalctl -u cortexfs-channel@discord.service -f
```

The adapter keeps one bounded WebSocket connection and uses the canonical
public `/ctx/agent/<agent>.sock` endpoint for the existing durable session ABI.
It validates that the endpoint is a live Unix socket before connecting. It does not add a
`/ctx/channel` namespace, watcher, or polling worker.

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

Slack and Feishu/Lark retain the explicit webhook ingress mode:

```bash
export CORTEXFS_CHANNEL_PLATFORM=slack
export CORTEXFS_CHANNEL_OUTBOUND_URL='https://slack.com/api/chat.postMessage'
export CORTEXFS_CHANNEL_BIND=127.0.0.1:8765
cortexfs-channel webhook
```

For Slack, use `https://slack.com/api/chat.postMessage` as the outbound URL and
set `CORTEXFS_CHANNEL_TOKEN` to a bot token. For Feishu/Lark, use the tenant
`im/v1/messages` endpoint and set the bearer token as required by the tenant.
`{path}` in `CORTEXFS_CHANNEL_OUTBOUND_URL` is replaced with the codec's
relative API path when a deployment wants one URL template for a codec.

Keep non-Discord credentials in the service manager's environment or secret
store. Do not put tokens in `/ctx`, a repository, command-line arguments, or
durable session metadata.

## Multi-turn and agent capability

The bridge derives one stable session from channel, conversation, and thread.
It submits text to the existing `agent/<name>.sock` with `scope=private`, so
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
