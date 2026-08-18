# cortexfs-channel-nostr

Process-isolated Nostr channel driver for CortexFS. It supports NIP-04
encrypted direct messages and NIP-17 gift wraps over relay WebSockets, then
bridges text messages to the runtime through `cortexfs.channel.socket/v1`.

The driver deliberately does not depend on FUSE, `/ctx`, or an Agent
implementation. Configure the runtime driver with `cortexfs-channel driver`,
then start this process with:

```text
CORTEXFS_NOSTR_PRIVATE_KEY=<secret key>
CORTEXFS_NOSTR_RELAYS=wss://relay.example.org
CORTEXFS_NOSTR_ALLOWED_USERS=npub1...
CORTEXFS_CHANNEL_SOCKET=/run/cortexfs/channel/nostr.sock
```

An empty sender allowlist denies inbound users. Keys remain in process memory;
the driver never writes credentials to the filesystem ABI or session history.
