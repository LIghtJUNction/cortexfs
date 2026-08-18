# cortexfs-channel-slack

Process-isolated Slack Socket Mode driver for CortexFS. It uses the common
`cortexfs.channel.socket/v1` Unix-socket ABI, keeps Slack tokens outside the
message ABI, and supports inbound events, proactive delivery, threads,
reactions, edits, and deletes.

Required environment:

```text
CORTEXFS_SLACK_APP_TOKEN=xapp-...
CORTEXFS_SLACK_BOT_TOKEN=xoxb-...
CORTEXFS_CHANNEL_SOCKET=/run/cortexfs/channel/slack.sock
```

The driver uses the canonical socket path returned by `cortexfs-paths` and
does not create a `/ctx/channel` namespace or a TCP listener.
