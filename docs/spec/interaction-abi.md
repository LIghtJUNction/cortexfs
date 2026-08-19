# Interaction ABI

`cortexfs-runtime-client` defines the provider-neutral interaction contract
shared by terminal, web, and channel frontends. It is a logical protocol; the
current transport is the existing agent/session Unix socket and its compatible
`send`, `resume`, `status`, and `cancel` operations.

The version marker is `cortexfs.interaction/v1`. A frame is one newline-
terminated JSON value:

```json
{
  "abi": "cortexfs.interaction/v1",
  "payload": {
    "direction": "request",
    "value": {
      "type": "input",
      "request_id": "web-1",
      "session": "default",
      "scope": "private",
      "input": "hello",
      "origin": {"transport": "web"}
    }
  }
}
```

An `input` request may also carry an optional provider-neutral `event` object.
Channel runtimes use it for reactions, typing, edits, deletes, and read
receipts; existing text inputs omit the field and remain wire-compatible.
The same bounded object is forwarded in the executable-agent invocation
envelope, so an Agent can inspect structured event data without learning a
platform-specific type.

Requests cover input, replay, status, cancellation, and replies to
runtime-initiated commands. Events normalize acceptance, start, deltas,
messages, tools, approval commands, status, errors, and completion. Every event carries
the frontend request id and, where applicable, a run id. A runtime command is
answered with `command_result`; this makes both directions independently
correlatable without letting a frontend call a tool directly.

The Unix-socket client keeps one write handle beside its bounded event reader.
When an `approval_request` is normalized as a `command` event, the client may
write this response before reading the next event:

```json
{
  "abi": "cortexfs.interaction/v1",
  "payload": {
    "direction": "request",
    "value": {
      "type": "command_result",
      "request_id": "web-1",
      "session": "default",
      "command_id": "call-1",
      "result": {"type": "accepted"}
    }
  }
}
```

`InteractionOrigin` is intentionally generic. It can carry transport,
endpoint, external identity, conversation, thread, and bounded metadata, but
does not define Telegram, Discord, HTTP, or provider-specific message types.
Identity resolution and permission checks remain runtime responsibilities.

The built-in web host accepts and returns these exact frames as JSONL. A
browser client therefore consumes the same request/event model as `ctxchat`
and a channel bridge; only the outer HTTP connection differs. Because the
current endpoint is one HTTP POST, it emits a command event but rejects an
interactive command with a bounded reason; a WebSocket or bidirectional
NDJSON endpoint is required for browser-side approval/input replies.

## Two protocol layers

The interaction ABI is the frontend/runtime layer:

```text
terminal / web / IM
        |  cortexfs.interaction/v1
        v
agent session runtime
```

`cortexfs-channels` separately defines `cortexfs.channel.socket/v1` for a
channel driver/runtime boundary. It carries channel lifecycle, inbound events,
message delivery, correlated effects (typing, reaction, edit, delete, mark
read), receipts, health, and reconnect events:

```text
platform adapter
        |  cortexfs.channel.socket/v1
        v
channel runtime -- interaction ABI --> agent session
```

Platform codecs remain below this boundary. They translate native payloads into
the existing `InboundMessage`/`OutboundMessage` ABI and never enter Agent code.

The interaction protocol creates no `/ctx/interaction` namespace. Channel
state/tools use the separate explicit `/ctx/channel` ABI and never become
interaction objects.
Durable history remains under the existing session path; sockets are live
transports and files remain the observable state surface.

The Rust traits in these crates are compile-time APIs, not promises of a stable
Rust binary ABI. External implementations should use the documented JSONL
socket frames or an independently versioned executable process.
