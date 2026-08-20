# cortexfs-channel-sdk

Rust host SDK for process-isolated CortexFS channel adapters.

Implement `ChannelService` for the platform-specific transport, connect a
`ChannelRuntime` to the runtime-owned Unix socket, and use its cloneable
`ChannelSender` from the platform receive loop. The SDK performs the stable
`cortexfs.channel.socket/v1` handshake and dispatches outbound messages, live
effects, runtime commands, health probes, lifecycle events, and shutdown.

Platform credentials, API payloads, HTTP/WebSocket clients, rate limiting, and
idempotency stay inside the adapter. Only provider-neutral channel values cross
the socket. Wire errors returned to CortexFS are bounded and do not include the
adapter's original error text.
