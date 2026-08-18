# cortexfs-channel-wecom-ws

Process-isolated WeCom AI Bot WebSocket driver for CortexFS. It keeps the
WeCom subscription secret, reconnect loop, heartbeat, allowlist, and streaming
reply frames outside `cortexfs-channels`; the only runtime boundary is
`cortexfs.channel.socket/v1` over the canonical Unix socket.

```dotenv
CORTEXFS_WECOM_BOT_ID=read-from-a-secret-store
CORTEXFS_WECOM_SECRET=read-from-a-secret-store
CORTEXFS_WECOM_ALLOWED_USERS=userid-a,userid-b
CORTEXFS_WECOM_ALLOWED_GROUPS=chatid-a
CORTEXFS_CHANNEL_SOCKET=/run/cortexfs/channel/wecom-ws.sock
```

The driver currently normalizes text callbacks, private/group identities, and
bounded streamed text replies. Unsupported media is rejected explicitly.
