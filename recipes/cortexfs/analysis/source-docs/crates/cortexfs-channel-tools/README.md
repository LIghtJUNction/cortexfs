# cortexfs-channel-tools

Tool SDK executable for the generic channel capability set.

The binary is installed once as `cortexfs-channel-tool`; the reference tree
exposes it under every `/ctx/channel/<name>/tool/` entry. Runtime selects the
object name from `CTX_AUTHORIZED_OBJECT` and sends a provider-neutral
`ControlRequest` through the channel driver socket.

Common tools include:

```text
channel.send channel.reply channel.typing channel.preview channel.react
channel.edit channel.delete channel.mark_read channel.pin channel.unpin
channel.redact channel.choice channel.multi_choice channel.input channel.ask
channel.approval channel.escalate channel.poll channel.notify channel.room_create
channel.room_invite channel.draft channel.draft_update channel.gate channel.forge
<channel>.invoke
```

Each channel directory also contains named adapter operations derived from the
ZeroClaw channel surface, for example `telegram.send_photo`,
`discord.send_embed`, `slack.send_blocks`, `email.search`, `gmail.read`,
`matrix.create_room`, and `git.forge_request`. The complete list is exposed by
`cortexfs_channels::ChannelSpec::platform_tool_names()`.

Every named operation uses the same `ChannelCommand::Invoke { name, payload }`
wire shape. The adapter owns validation, credentials, retries, and the native
API call; this executable never contains Discord, Telegram, or provider
credential types.
