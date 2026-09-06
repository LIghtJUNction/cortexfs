# cortexfs-protocol

`cortexfs-protocol` is a provider-neutral Rust protocol-conversion crate for
AI/LLM APIs. It is designed for OpenAI Chat Completions, the OpenAI Responses
API, Google Gemini `generateContent`, Anthropic Messages, tool calling,
multimodal content, reasoning controls, response usage, and streaming-oriented
event normalization.

Search keywords: AI protocol conversion, LLM protocol adapter, OpenAI Chat
Completions, OpenAI Responses API, Gemini API, Anthropic Messages API, function
calling, tool use, multimodal, zero-copy, Rust IR, CortexFS.

```toml
cortexfs-protocol = "0.1.7"
```

## Design

This crate is the protocol/AI layer in CortexFS’s Pi-aligned package map
([architecture.md](../../docs/architecture.md)): provider-neutral IR only.
It deliberately has multiple protocol-specific borrowed wire IRs plus
an owned semantic IR. It is not a JSON-only “one universal struct”:

| Layer | IR | Purpose |
| --- | --- | --- |
| Native wire IR | `openaichat`, `openairesponses`, `gemini`, `anthropic` | Preserve dialect-specific fields and borrow input strings/raw JSON. |
| Semantic request IR | `ModelRequest` | Normalize messages, content parts, tools, choices, limits, options, and context ownership. |
| Semantic event IR | `ModelEvent` | Normalize text/reasoning deltas, tool calls, usage, errors, and terminal state. |

The caller always supplies the source/target `WireProtocol`; the semantic IR
does not guess whether the input was Chat Completions or Responses. This is
important because protocol behavior is not interchangeable even when both
accept “messages”.

## Request conversion matrix

All 4 × 4 request directions are supported, including identity routes:

| Source → target | Same dialect | Lossless direct route | Semantic IR fallback |
| --- | ---: | ---: | ---: |
| OpenAI Chat | identity | Gemini | Responses, Anthropic |
| OpenAI Responses | identity | — | Chat, Gemini, Anthropic |
| Gemini | identity | OpenAI Chat | Responses, Anthropic |
| Anthropic Messages | identity | — | Chat, Responses, Gemini |

`transcode_request` chooses the direct route first. The current direct adapter
is the common text/image/function-schema subset of OpenAI Chat ↔ Gemini; it
borrows unescaped source fields and avoids constructing `ModelRequest`. Other
directions decode a native IR, build the owned semantic IR, and encode the
target dialect. Unsupported or provider-owned fields fail explicitly rather
than being silently discarded.

```rust
use cortexfs_protocol::{
    BridgePath, WireProtocol, transcode_request,
};

let converted = transcode_request(
    WireProtocol::OpenAiChat,
    WireProtocol::Gemini,
    br#"{"model":"gemini-2.5-pro","messages":[{"role":"user","content":"hi"}]}"#,
)?;
assert_eq!(converted.path, BridgePath::Direct);
```

For explicit semantic control:

```rust
use cortexfs_protocol::{
    WireProtocol, decode_model_request, encode_model_request,
};

let request = decode_model_request(WireProtocol::OpenAiResponses, input)?;
let target_json = encode_model_request(WireProtocol::Anthropic, &request)?;
```

## Context ownership is semantic metadata

`ModelRequest.context` carries:

- `ClientOwned`: the client supplies the complete history;
- `ProviderOwned`: the provider owns history behind an opaque reference;
- `Hybrid`: both a reference and materialized history may be used;
- `ReplayPolicy`: `full_history`, `materialize_history`, or `reference_only`;
- `ContextReference { namespace, value }` for provider-scoped IDs.

When an OpenAI Responses request contains `previous_response_id` or
`conversation`, decoding sets `ProviderOwned` and retains the namespace. A
Chat/Gemini/Anthropic encoder refuses that opaque Responses reference instead
of pretending it is portable. The caller must materialize history or select a
Responses-compatible target. Chat, Gemini, and Anthropic requests default to
`ClientOwned`.

## Response and event conversion

`decode_response_events` and `encode_response_events` normalize complete JSON
responses for all four dialects. `transcode_response` converts any response
pair through `ModelEvent`, preserving text, reasoning, tool calls, usage, and
terminal status. Identity response routes preserve bytes exactly. The event IR
is also the boundary used by streaming adapters; native SSE/NDJSON framing is
owned by the transport adapter rather than this crate.

```rust
use cortexfs_protocol::{WireProtocol, transcode_response};

let response = transcode_response(
    WireProtocol::Anthropic,
    WireProtocol::OpenAiResponses,
    anthropic_json,
)?;
```

## Zero-copy boundary

`decode_native_request` returns a protocol-specific borrowed IR. With ordinary
unescaped JSON strings, model names, message text, tool schemas, and raw JSON
values point into the caller’s input buffer (`Cow::Borrowed` / `RawValue`).
The direct Chat ↔ Gemini route retains this property while serializing the
target. Semantic fallback necessarily allocates owned strings because it must
change ownership and normalize different schemas; it does not claim to be
zero-copy.

No HTTP client, API key storage, provider SDK, background task, or unsafe code
is included. The crate only parses, converts, validates, and serializes data.

## Correctness tests

The integration suite exercises borrowed native IR, request identity/direct/
fallback routes, the request conversion matrix, Responses context ownership,
semantic validation, response dialects, and response conversion directions.
Run it against the current revision:

```sh
scripts/serialize-cargo.sh cargo test --locked -p cortexfs-protocol
scripts/serialize-cargo.sh cargo clippy --locked -p cortexfs-protocol --all-targets -- -D warnings
```

The former conversion timing example compared different target formats. It was
removed because those timings do not isolate the cost of direct versus semantic
conversion. Historical observations remain in Git history. Harness contract
verification is documented in [Harness evaluation](../../docs/evaluation.md).

## Versioning and publishing

The crate follows the CortexFS workspace version (`0.1.7` in this release).
The native IR modules and semantic schema are public ABI surfaces; additions
should be backward-compatible within a minor release. Package locally with:

```text
cargo package --locked -p cortexfs-protocol --allow-dirty
```
