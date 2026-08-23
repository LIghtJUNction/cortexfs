# Module ABI

This document defines two deliberately different boundaries for CortexFS
runtime extensions. The implementation is the independently usable
`cortexfs-module` crate.

The Rust trait is a static module API, not a stable binary ABI. Rust does not
promise a stable trait-object, vtable, `String`, allocator, panic, or async
future layout across compiler versions, targets, or dynamic libraries. The
stable external boundary is the versioned JSONL wire contract described below.

## Scope

The module ABI gives Agent, Tool, Channel, Model, and Context extensions one
common identity, capability, and lifecycle surface. It is the agent-core
extension edge in the Pi-aligned architecture map
([architecture.md](../architecture.md)): modules plug into lifecycle and
capabilities; they do not invent root classes or own the frontend.

```text
CortexFS Runtime
      |
      +-- cortexfs-module
            +-- Agent
            +-- Tool
            +-- Channel
            +-- Model
            +-- Context
```

It does not define a new `/ctx` class, provider message type, session store,
FUSE operation, or external platform protocol. Domain SDKs remain responsible
for their own typed behavior and the runtime remains responsible for policy,
routing, durability, and process ownership.

## Metadata and capabilities

Every module exposes `ModuleMetadata`:

```rust
ModuleMetadata::new("channel.example", "1.0.0", ModuleKind::Channel)
    .with_capability("text", "send and receive text")
```

`id` and `version` identify the implementation. `kind` is one of `Agent`,
`Tool`, `Channel`, `Model`, or `Context`. Capability names are
provider-neutral declarations; they do not grant permission. Policy and the
domain ABI still decide whether a capability can be used.

The static Rust identifier is `cortexfs.module/v1`. It versions the typed host
API inside one Cargo build; it does not make Rust trait objects safe to load
from `.so` files.

## Lifecycle

`CortexModule` has four executor-independent asynchronous operations:

```text
Registered -> Initialized -> Running -> Stopped -> Shutdown
```

The host supplies only a `ModuleContext` containing its runtime instance
identifier. The module does not receive FUSE handles, `/ctx` paths, secrets,
or an Agent callback from this ABI. A `ModuleRegistry` registers modules by
stable id and drives them in deterministic id order.

Lifecycle failures use `ModuleError`; hosts may wrap them in the wider runtime
diagnostic system without losing the module id or operation boundary.

## External process wire ABI

An external module is a separately supervised process connected through a
runtime-owned Unix socket. The socket carries one newline-terminated JSON
object per frame, with a maximum encoded frame size of 1 MiB. Its version is
`cortexfs.module.socket/v1`; unknown JSON fields are ignored for additive
forward compatibility, while an unknown `type` or wrong `abi` is rejected.

The initial handshake is host-to-module `hello`, followed by module-to-host
`ready`:

```json
{"type":"hello","abi":"cortexfs.module.socket/v1","instance":"agent-1"}
{"type":"ready","metadata":{"id":"channel.example","version":"1.0.0","kind":"channel","capabilities":[]}}
```

The host drives `lifecycle` frames (`init`, `start`, `stop`, `shutdown`).
Domain SDKs use provider-neutral `call`/`result` frames, and modules publish
`event` frames. `error` frames carry a stable code and a bounded diagnostic
message; secrets and prompt contents do not belong in them. Large data such as
attachments must use an existing file or object ABI and be referenced by the
domain payload rather than expanding the socket frame.

The wire contract is a serialization and framing ABI, not a promise that every
module can call every subsystem. Runtime policy, Linux credentials, object
receipts, and existing `/ctx`/session socket authority remain in the host.

## Runtime and Unix ABI relationship

The module is code, not a filesystem object. Runtime state continues to use
the existing files and sockets:

```text
module code -> runtime registration/lifecycle
durable state -> existing agent/session files
live control/events -> existing agent/session sockets
```

No `/ctx/module` or `/ctx/plugin` namespace is introduced. The explicit
`/ctx/channel` root is owned by the channel subsystem, not by the Module ABI.
An adapter may expose ordinary object-local or session-local status where an
existing ABI already permits it.

## Loading boundary

The Rust trait is intended for static composition and Cargo feature selection.
The external process/socket contract is the recommended extension boundary for
third-party modules because it also provides process identity and failure
isolation. A future native in-process loader would need a separate C ABI with
`#[repr(C)]`, opaque handles, explicit ownership functions, and a handshake;
it must not expose Rust trait objects. WASM/WIT is another possible sandboxed
boundary. Neither transport changes the core module metadata and lifecycle
model.
