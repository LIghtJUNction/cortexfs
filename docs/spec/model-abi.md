# Model ABI

There is only one model ABI:

```text
/ctx/model/<provider>/<model>       one-shot inference executable
/ctx/model/<provider>/<model>.sock  optional CortexFS session socket
/ctx/model/<provider>/<model>.d/    control files
/ctx/model/main                     default model symlink
/ctx/model/helper                   helper model symlink
```

`<provider>/<model>` is represented as two path components. For native model
providers, `<provider>` is the original provider identity:

```text
/ctx/model/openai/gpt-4o
/ctx/model/anthropic/claude-sonnet-4
/ctx/model/google/gemini-2.5-pro
```

For a custom base URL without a declared original provider mapping,
`<provider>` is the normalized host name. For example,
`https://api.lmm.best:9000/` projects models under:

```text
/ctx/model/api.lmm.best/gpt-5.4-mini
```

The custom base URL is provider-adapter configuration, not a root ABI namespace.
It may be shown in `model/<provider>/<model>.d/default` for inspection, but
secrets never appear in model metadata or `.d/` files.

Bottom-layer AI API formats do not enter the ABI. OpenAI Responses, Anthropic
Messages, Gemini GenerateContent, OpenAI-compatible chat, local runtimes, and
aggregator-specific request formats are Rig or provider-adapter details.
Bottom-layer stateful/stateless behavior does not enter the ABI. Rig adapts
provider connections, API compatibility, and streaming into the canonical
CortexFS request and JSONL event stream.

Example:

```text
/ctx/model/
  main -> /ctx/model/debug/echo
  helper -> /ctx/model/debug/echo
  debug/
    echo
    echo.d/
      id
      driver
      cap
      default
      session
      status
      log
  openai/
    gpt-4o
    gpt-4o.d/
      id
      driver
      cap
      default
      session
      status
      log
```

Control files:

```text
id       provider-native model id or runtime-internal model id
driver   driver route table; see below
cap      capability list, one per line
default  default parameters, KEY=VALUE, one per line
session  none or socket
status   dynamic status
log      short call log or pointer to log location
```

`driver` may be a legacy single driver name:

```text
debug
```

or a route table:

```text
default=openai-chat
exec=openai-chat
socket=openai-chat
agent=openai-responses,openai-chat
```

Route keys:

```text
default  fallback route
exec     direct one-shot model file execution
socket   direct model socket calls
agent    agent-owned model calls
```

Each value is a comma-separated priority list. Runtime selection checks the
use-case route first, then `default`. This lets direct model usage choose a
classic chat driver while agents prefer a richer Responses-style driver with a
chat fallback. Driver names are adapter names, not stable model names.

Secrets are never stored in model files or `.d/` control files. Provider
credentials use this priority:

```text
environment variable
system keychain
unconfigured
```

For example, a provider adapter may first read `LMM_API_KEY`, then look up a
system keychain item such as `service=cortexfs:lmm account=default`. If both
are absent, the model is not configured and must return a stable error.

OAuth providers use the same rule: access tokens are bearer credentials and
remain provider-runtime state, not model ABI state. A provider config may
declare OAuth Authorization Code + PKCE metadata:

```json
{
  "base_url": "https://api.example.com/v1",
  "oauth": {
    "client_id": "cortexfs-local",
    "auth_url": "https://auth.example.com/oauth/authorize",
    "token_url": "https://auth.example.com/oauth/token",
    "redirect_uri": "http://127.0.0.1:8765/callback",
    "scopes": ["model.read", "offline_access"],
    "access_token_env": "EXAMPLE_OAUTH_ACCESS_TOKEN",
    "refresh_token_env": "EXAMPLE_OAUTH_REFRESH_TOKEN"
  }
}
```

`access_token_env` is checked before the system keychain. If it is absent or
empty, the runtime looks up `service=cortexfs:<provider> account=oauth:access`.
Refresh tokens, when used by a future provider adapter or CLI wrapper, use
`account=oauth:refresh` by default. PKCE uses `S256`; the verifier and callback
state are short-lived local flow state and must not be written into `/ctx/model`.

## One-Shot Exec

`/ctx/model/<provider>/<model>` is a read-only executable object. Reading it returns
CortexFS metadata text for that model. Executing it performs one-shot
inference through CortexFS/Rust runtime code or a provider adapter; model
objects must not be shell-script implementations.

The first metadata keys mirror Rig 0.39 model listing fields:

```text
id
name
description
type
created_at
owned_by
context_length
```

Provider adapters may populate those fields from
`ModelListingClient::list_models()` / `ModelList`. Built-in `debug/*` models
are local debug metadata and do not imply a provider default.

```bash
/ctx/model/debug/echo "hello"
echo "hello" | /ctx/model/openai/gpt-4o
echo '{"messages":[{"role":"user","content":"hello"}]}' | /ctx/model/openai/gpt-4o
```

Semantics:

```text
one invocation
no durable session mutation
stdout is the canonical JSONL event stream
exit code is the process-level summary
file content is inspectable metadata, not provider code or secrets
```

Even if the underlying provider has native state,
`/ctx/model/<provider>/<model>` behaves as a stateless single call.

## Provider Transport Routes

Proxying is provider transport configuration, not an agent and not a second
model namespace. The provider must already exist, the model id stays under that
provider, and the runner selects a transport before sending the provider-native
request.

Example provider config:

```json
{
  "base_url": "https://api.openai.com/v1",
  "api_key_env": "OPENAI_API_KEY",
  "transports": {
    "office-http": {
      "kind": "http",
      "url": "http://127.0.0.1:8080/v1"
    },
    "local-socket": {
      "kind": "unix",
      "path": "/run/user/1000/cortexfs/proxy/openai.sock",
      "url": "http://localhost/v1"
    }
  },
  "route": [
    {
      "model": "gpt-4o",
      "transport": "office-http"
    },
    {
      "model": "embedding-*",
      "transport": "local-socket"
    }
  ]
}
```

`route[].model` supports exact model ids and trailing `*` prefixes. If no route
matches and `default_transport` is absent, the runner uses `base_url` directly.
This lets many models share one HTTP or Unix-socket proxy without creating
fake provider names or debug agents. Secrets still resolve through the provider
environment/keychain path; transport entries only decide where bytes are sent.

## Model Socket

`/ctx/model/<provider>/<model>.sock` is the only multi-turn model entry. It
uses the shared JSONL socket protocol from [object-abi.md](object-abi.md).

Examples:

```jsonl
{"op":"send","id":"msg-1","session":"default","input":"hello"}
{"op":"resume","session":"default","after":"event-123"}
{"op":"cancel","id":"run-1"}
{"op":"ping"}
```

A model socket session is CortexFS session semantics, not provider-native
session semantics. Native threads, response ids, context caches, and simulated
message logs are hidden behind the canonical protocol.

`model/<provider>/<model>.d/session` has only two stable values:

```text
none    no /ctx/model/<provider>/<model>.sock
socket  /ctx/model/<provider>/<model>.sock exists and supports CortexFS sessions
```

The value never describes provider-native state.

## Capabilities

Use stable semantic capability words:

```text
chat
stream
session
vision
audio_input
audio_output
json_schema
tool_call_syntax
reasoning
embedding
rerank
```

Provider-private or API-format-private capability words are forbidden in stable
ABI:

```text
openai_responses
anthropic_messages
gemini_generate_content
native_thread
native_stateful
native_stateless
```

`tool_call_syntax` only means the model event stream may contain
tool-call-shaped events. It does not mean the model can execute tools. It
grants no tool permission.

## Canonical Event Stream

v1 model and agent streams use these event types:

```text
start
delta
message
reasoning_delta
reasoning_message
tool_call
usage
error
done
```

Example:

```jsonl
{"type":"start","run":"r1","model":"debug/echo"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"message","run":"r1","role":"assistant","content":[{"type":"text","text":"hello"}]}
{"type":"usage","run":"r1","input_tokens":10,"output_tokens":1}
{"type":"done","run":"r1","status":"ok"}
```

Error example:

```jsonl
{"type":"error","run":"r1","code":"EACCES","message":"permission denied"}
{"type":"done","run":"r1","status":"error"}
```

`code` uses stable errno names. Clients must not parse `message`.

## Native Diagnostics

`model/<provider>/<model>.d/native` may exist for diagnostics only:

```text
native is diagnostic only
native is not stable ABI
strict clients must not depend on it
```

## Tool Boundary

Model execution is not tool execution.

Hard rule:

```text
model may emit tool_call events
model must not execute tools
agent decides whether to execute tools
agent policy decides whether execution is allowed
```

Model processes must not receive project mounts, tool credentials, or write
access outside runtime-owned cache. Provider tool calling must not become a
backdoor around agent policy.
