# Model ABI

There is only one model ABI:

```text
/ctx/model/<provider>/<model>       one-shot inference executable
/ctx/model/<provider>/<model>.sock  optional CortexFS session socket
/ctx/model/<provider>/<model>.d/    control files
/ctx/model/main                     default coder model symlink
/ctx/model/helper                   default reviewer model symlink
```

`<provider>/<model>` is represented as two path components. For native model
providers, `<provider>` is the original provider identity:

```text
/ctx/model/openai/gpt-4o
/ctx/model/anthropic/claude-sonnet-4
/ctx/model/google/gemini-2.5-pro
```

For a custom domain base URL without a declared original provider mapping,
`<provider>` is the normalized host name. For example,
`https://api.lmm.best:9000/` projects models under:

```text
/ctx/model/api.lmm.best/gpt-5.4-mini
```

Address-like endpoints such as `127.0.0.1`, `::1`, or `localhost` MUST set an
explicit provider `name` in the host-side provider config. Without that name,
the config is invalid because `/ctx/model/<provider>` must be a stable object
name, not a transport address. For example:

```json
{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
```

This projects as `/ctx/model/local/gpt-5.4-mini`.

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
  main -> /ctx/model/openai/gpt-5.5
  helper -> /ctx/model/openai/codex-auto-review
  debug/
    echo
    echo.d/
      id
      driver
      cap
      effort
      default
      fallback
      limit
      session
      status
      log
  openai/
    gpt-4o
    gpt-4o.d/
      id
      driver
      cap
      effort
      default
      fallback
      session
      status
      log
```

Control files:

```text
id       provider-native model id or runtime-internal model id
driver   driver route table; see below
cap      capability list, one per line
effort   provider-neutral reasoning effort: auto, low, medium, high, or xhigh
default  default parameters, KEY=VALUE, one per line
fallback ordered fallback model chain, one provider/model name per line
limit    maximum hard context size in tokens, or unknown
session  none or socket
status   dynamic status
log      short call log or pointer to log location
```

## Hard Context Limit

Every model control directory contains a read-only `limit` file:

```text
/ctx/model/<provider>/<model>.d/limit
```

The file contains exactly one canonical LF-terminated line. Its value is
either `unknown` or a positive base-10 `u32` token count. Numeric values use no
sign, surrounding whitespace, or leading zeroes. Zero, overflow, extra lines,
and non-canonical decimal text are invalid. The number is the provider/model's
hard combined context limit; it is not an output-token setting and must not be
used as one.

Examples:

```text
272000
unknown
```

`unknown` means CortexFS has no trusted maximum. It must not be rendered as
zero or replaced with a guessed value. The executable model metadata field
`context_length` contains the same canonical value as `limit`.

`limit` is an inspectable projection, never an Agent-writable control. FUSE
opens and writes that request mutation must fail with `EROFS`, including for
uid 0. Updating a limit happens only when host configuration changes or during
the existing synchronous mount-start catalog refresh; there is no watcher,
poller, or hot-reload path.

The resolver uses this precedence:

```text
1. model_limits in the selected host provider config
2. a valid CortexFS-owned models.dev cache entry
3. unknown
```

A provider config may declare explicit limits for locally configured models
without changing the backward-compatible string `models` list:

```json
{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "models": ["custom-model"],
  "model_limits": {
    "custom-model": 32768
  }
}
```

Each `model_limits` key must be a model listed by `default_model` or `models`,
and each value must be in `1..=4294967295`. Invalid local limit declarations
make that provider config invalid; they are not silently ignored. A local
entry overrides catalog data for the same projected model.

CortexFS obtains catalog limits through the external `models-dev` library.
Catalog provider and model map keys are matched exactly to the projected
`<provider>/<model>` identity; transport hosts and aggregator names are not
guessed as original providers. Only stable CortexFS provider/model names and
positive limits enter the cache.

The host cache is bounded, versioned data with this shape:

```json
{
  "schema": "cortexfs.model-limits/v1",
  "models": {
    "openai/gpt-5.5": 272000
  }
}
```

The cache is atomically replaced only after a complete successful online
response has been parsed and validated. A timeout, network error, invalid or
oversized response, empty validated result, or failed durable write preserves
the last valid cache unchanged. A missing, malformed, oversized, wrong-schema,
or unsafe cache supplies no limit. Catalog cache content contains no provider
credentials and is backend state, not a new `/ctx` namespace.

`fallback` is a model fallback chain, not a transport route. It lives next to
the selected model in `model/<provider>/<model>.d/fallback`; each non-comment
line is another stable provider/model reference, for example:

```text
openai/codex-auto-review
api.lmm.best/gpt-5.3-codex-spark
```

When the selected model is unavailable or fails before producing a successful
answer, the runtime tries fallback models in order. Each candidate still uses
the normal provider registry, secret lookup, and `/ctx/model/route` egress
rules.

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
root-owned CortexFS system secret store
unconfigured
```

The API key is read from
`/var/lib/cortexfs/secrets/provider/<provider>/<slot>`. Provider JSON must not
declare API-key environment variable names, and API keys must not be placed in
process environments. If the system secret is absent, the model is not
configured and must return a stable error unless the endpoint supports
unauthenticated requests.

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
    "scopes": ["model.read", "offline_access"]
  }
}
```

OAuth token environment names are generated from provider identity, for example
`CTX_EXAMPLE_OAUTH_ACCESS_TOKEN` and `CTX_EXAMPLE_OAUTH_REFRESH_TOKEN`; users do
not configure those names in provider JSON. If the generated access-token
variable is absent or empty, the runtime looks up
`service=cortexfs:<provider> account=oauth:access`. Refresh tokens, when used by
a provider adapter or CLI wrapper, use `account=oauth:refresh` by default. PKCE
uses `S256`; the verifier and callback state are short-lived local flow state
and must not be written into `/ctx/model`.
`ctx provider oauth login PROVIDER` is the host-side helper that performs this
PKCE login flow and writes tokens to the system keychain.

## Provider Presets

Provider presets are host-side JSON file templates. They install under
`/etc/cortexfs/providers.d/` and do not create a `/ctx/provider` namespace:

```text
ctx provider preset list
ctx provider preset show openai|codex|anthropic|google
ctx provider preset install openai|codex|anthropic|google
```

Canonical provider names:

```text
openai     OpenAI API with `/v1/responses` for agent calls and
           `/v1/chat/completions` fallback; `codex` is an alias
anthropic  Claude Messages API
google     Gemini through Google's OpenAI-compatible endpoint; `gemini` is an alias
```

The `codex` alias installs the OpenAI preset and projects Codex-recommended
OpenAI models under the canonical provider path, for example
`/ctx/model/openai/gpt-5.5`. It does not create `/ctx/model/codex` or a second
provider namespace.

The Google preset uses Gemini's OpenAI-compatible endpoint. The Anthropic
preset uses `anthropic.messages`, so the runner sends `POST /v1/messages` with
the required Anthropic version header.

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

## Global Model Route

Model proxying is not an agent and is not stored in provider JSON. The single
global egress route table is:

```text
/ctx/model/route
```

The file is ordinary CortexFS state. It is read only when a model request is
made; if the file is absent, the projected default is `fallback: direct`.

Rules are evaluated top to bottom. A rule selects a group; a group selects both
transport and an optional credential slot. Secrets are never written into the
route file or provider JSON. `key(NAME)` selects
`/var/lib/cortexfs/secrets/provider/<provider>/NAME` from the CortexFS system
secret store. API keys are not placed in process environments. Without
`key(...)`, the default credential slot is `default`.

```text
group(proxy) -> http(http://127.0.0.1:8080/v1), key(office)
group(local-socket) -> unix(/run/user/1000/cortexfs/proxy/openai.sock), key(local)

dip(198.51.100.45) -> direct
# dip(203.0.113.43) -> JP
domain(bestproxy.com) -> proxy
pname(NetworkManager, systemd-resolved, dnsmasq) -> must_direct
dip(geoip:private) -> direct
dip(geoip:cn) -> direct
domain(geosite:cn) -> direct
model(embedding-*) -> local-socket
fallback: proxy
```

Built-in group names:

```text
direct       use the provider base_url and default credential slot
must_direct  same transport as direct, intended for policy readability
```

Custom groups use:

```text
group(NAME) -> direct[, key(SLOT)]
group(NAME) -> http(BASE_URL)[, key(SLOT)]
group(NAME) -> unix(SOCKET_PATH[, BASE_URL])[, key(SLOT)]
```

Matchers currently include `domain(...)`, `dip(...)`, `pname(...)`,
`provider(...)`, and `model(...)`. `model(...)` and `provider(...)` accept
exact names and trailing `*` prefixes.

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
