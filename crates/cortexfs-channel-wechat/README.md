# cortexfs-channel-wechat

Process-isolated WeChat personal iLink Bot driver for CortexFS. It long-polls
`getupdates`, maps each external user to a stable CortexFS conversation, and
uses the canonical `cortexfs.channel.socket/v1` Unix socket for agent delivery.

```dotenv
CORTEXFS_WECHAT_TOKEN=read-from-a-secret-store
CORTEXFS_WECHAT_ALLOWED_USERS=wxid-a,wxid-b
CORTEXFS_CHANNEL_SOCKET=/run/cortexfs/channel/wechat.sock
```

The driver implements text and voice-transcript messages, context-token
continuation, cursor-based polling, bounded retries, and text replies. QR
login/media upload remain explicit follow-up capabilities; the token must be
obtained through the WeChat iLink pairing flow before starting this process.
