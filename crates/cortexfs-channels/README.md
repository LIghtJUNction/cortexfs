# cortexfs-channels

Runtime-neutral multi-channel messaging abstractions for agent software.

The crate defines one message model, one adapter lifecycle, registry-based
routing, delivery receipts, health reporting, and a versioned JSON envelope.
It does not depend on CortexFS, FUSE, a model provider, an HTTP runtime, or a
particular IM platform. Platform codecs and host runtimes can be layered on
top without changing the agent-facing ABI.

```toml
cortexfs-channels = "0.1"
```

See the crate documentation for the adapter contract and the `platform`
module for reusable webhook codecs.
