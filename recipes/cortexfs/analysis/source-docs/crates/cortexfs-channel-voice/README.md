# cortexfs-channel-voice

Process-isolated voice channel driver for CortexFS. It implements the
provider-neutral `cortexfs.channel.socket/v1` boundary for ZeroClaw's
`voice_call` family (Twilio, Telnyx, and Plivo) and the Telnyx ClawdTalk
channel.

The driver accepts JSON or form-encoded provider webhooks, maps phone
identities to `call:<id>` conversations, and turns runtime outbound text into
provider text-to-speech actions. All destinations must be present in
`CORTEXFS_VOICE_ALLOWED_DESTINATIONS` (or the explicit `*` entry).

Example environment:

```dotenv
CORTEXFS_VOICE_CHANNEL=voice_call
CORTEXFS_VOICE_PROVIDER=telnyx
CORTEXFS_VOICE_AUTH_TOKEN=read-from-a-secret-store
CORTEXFS_VOICE_ACCOUNT_ID=connection-id
CORTEXFS_VOICE_FROM_NUMBER=+10000000000
CORTEXFS_VOICE_ALLOWED_DESTINATIONS=+10000000001
CORTEXFS_VOICE_WEBHOOK_BIND=127.0.0.1:8789
CORTEXFS_CHANNEL_SOCKET=/run/cortexfs/channel/voice_call.sock
```

Set `CORTEXFS_VOICE_CHANNEL=clawdtalk` for the Telnyx Call Control
integration; its socket is `/run/cortexfs/channel/clawdtalk.sock`.
