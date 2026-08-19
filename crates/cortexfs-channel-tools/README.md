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
channel.redact channel.choice channel.approval channel.notify
channel.room_create channel.room_invite channel.draft channel.gate channel.forge
<channel>.invoke
```

Platform-specific behavior remains in the adapter and uses the generic
`ChannelCommand::Invoke` payload; the tool crate never contains Discord,
Telegram, or provider credential types.
