# Model ABI

There is only one model ABI:

```text
/ctx/model/<provider>/<model>       one-shot inference executable
/ctx/model/<provider>/<model>.sock  optional CortexFS session socket
/ctx/model/<provider>/<model>.d/    control files
/ctx/model/main                     default model symlink
/ctx/model/{helper,fast,reason,code,vision}
                                    canonical compatibility/capability aliases
```

`<provider>/<model>` is represented as two path components. For native model
providers, `<provider>` is the original provider identity:

```text
/ctx/model/openai/gpt-5.6
/ctx/model/anthropic/claude-sonnet-5
/ctx/model/google/gemini-3.6-flash
```

For a custom domain base URL without a declared original provider mapping,
`<provider>` is the normalized host name. For example,
`https://models.example.test:9000/` projects models under:

```text
/ctx/model/models.example.test/compatible-model
```

Address-like endpoints such as `127.0.0.1`, `::1`, or `localhost` MUST set an
explicit provider `name` in the host-side provider config. Without that name,
the config is invalid because `/ctx/model/<provider>` must be a stable object
name, not a transport address. For example:

```json
{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "custom-model",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
```

This projects as `/ctx/model/local/custom-model`.

The custom base URL is provider-adapter configuration, not a root ABI namespace.
It may be shown in `model/<provider>/<model>.d/default` for inspection, but
secrets never appear in model metadata or `.d/` files.

Bottom-layer AI API formats do not enter the ABI. OpenAI Responses, Anthropic
Messages, Gemini GenerateContent, OpenAI-compatible chat, local runtimes, and
aggregator-specific request formats are protocol-adapter details.
Bottom-layer stateful/stateless behavior does not enter the ABI. CortexFS
protocol adapters convert provider connections, API compatibility, and
streaming into the canonical CortexFS request and JSONL event stream.

Example:

```text
/ctx/model/
  main -> /ctx/model/openai/gpt-5.6
  helper -> /ctx/model/openai/gpt-5.6-sol
  fast -> /ctx/model/openai/gpt-5.6
  reason -> /ctx/model/openai/gpt-5.6
  code -> /ctx/model/openai/gpt-5.6
  vision -> /ctx/model/openai/gpt-5.6
  debug/
    echo
    echo.d/
      id
      driver
      cap
      effort
      default
      limit
      recommended
      compact
      session
      status
      log
  openai/
    gpt-5.6
    gpt-5.6.d/
      id
      metadata.json
      driver
      cap
      effort
      default
      limit
      recommended
      compact
      session
      status
      log
```

`main`, `helper`, `fast`, `reason`, `code`, and `vision` are the complete
canonical alias set. `helper` remains a compatibility alias. Bootstrap may
select a projected model whose provider-neutral metadata matches a capability
alias; when it cannot identify one, that alias points to the selected `main`
target. Existing valid user-managed alias symlinks are preserved.

Control files:

```text
id       provider-native model id or runtime-internal model id
metadata.json complete normalized metadata plus the exact models.dev model object
driver   driver route table; see below
cap      capability list, one per line
effort   provider-neutral reasoning effort: auto, none, low, medium, high, xhigh, or max
default  default parameters, KEY=VALUE, one per line
limit    maximum hard context size in tokens, or unknown
recommended recommended working context size selected by metadata, or unknown
compact  context-compaction trigger selected by metadata, or unknown
session  none or socket
status   dynamic status
log      short call log or pointer to log location
```

The remaining control files describe the selected adapter rather than repeat
model metadata: `default` exposes the non-secret base endpoint used by the
executor, `session` declares whether a CortexFS model socket exists, `status`
reports the projection lifecycle, and `effort` is the provider-neutral default
reasoning effort. They are inspection and routing inputs; model facts such as
context limits and capabilities come from `limit`/`metadata.json`, while an
Agent's effective context choices live under `agent/<name>.d/`.

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

`recommended` and `compact` are read-only model policy projections. `limit` is
the trusted hard ceiling; `recommended` is the conservative working window
chosen by `cortexfs-metadatas`; `compact` is the token threshold at which a
context compiler should compact before the working window is exhausted. The
default metadata policy uses the smaller of the hard limit and 131072 tokens,
then compacts at 80 percent. A metadata record may provide more precise
recommendations, but both values are always bounded by `limit`.

`metadata.json` is a read-only JSON document owned by `cortexfs-metadatas`. Its
`metadata` object contains the normalized CortexFS fields (identity, aliases,
limits, modalities, capabilities, reasoning, lifecycle and provenance). When
the record came from the official models.dev catalog, `metadata.models_dev`
contains the complete upstream model object without dropping fields that this
crate does not yet normalize. This includes description, family, knowledge
cutoff, dates, attachment, reasoning options, interleaved reasoning,
structured output, temperature, modalities, open weights, limits and pricing.
The document's `effective` object reports the host projection after any
explicit provider override; it is distinct from the upstream hard limit.

`cap` is intentionally a stable positive capability index, not a dump of every
upstream field. `supported` facts become capability words; `unsupported` and
`unknown` facts remain visible in `metadata.json` and are not advertised as
usable. Transport capabilities that are not model facts, such as streaming on
the OpenAI Chat/Responses adapter, may also be added to `cap` after the model
facts are resolved. This keeps the Agent-facing file useful without losing the
complete upstream record.

The catalog refresh validates the current raw `models.dev` document directly.
A missing required field, identity mismatch, unsafe model id, oversized
response, or malformed cache is rejected atomically; the previous valid cache
remains in place. Optional upstream facts are retained exactly when present
and become `unknown` in the normalized Rust view when omitted.

Agents keep their effective choices in `agent/<name>.d/window` and
`agent/<name>.d/compact`. `auto` follows the selected model's `recommended`
and `compact` files; a positive explicit value is an intentional per-Agent
attenuation and may not exceed the applicable model/Agent ceiling. Thus model
metadata remains read-only while an Agent can safely choose a smaller budget.

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

Provider configuration may also override stable semantic capabilities for
individual declared models:

```json
{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "models": ["text-model", "vision-model"],
  "model_capabilities": {
    "text-model": ["chat", "stream"],
    "vision-model": ["chat", "stream", "vision"]
  }
}
```

Each key must name `default_model` or an entry in `models`. Values must be
unique stable capability words from the list below. Provider-private, unknown,
or duplicate words make the provider configuration invalid. An explicit empty
list is valid and projects an empty `cap` file. Models without an override use
the adapter-derived capability projection.

CortexFS obtains the catalog from the raw `models.dev/api.json` endpoint.
The refresh path retains each model object verbatim in `metadata.models_dev`
and publishes it through the read-only `metadata.json` file interface.
Catalog provider and model map keys are matched exactly to the projected
`<provider>/<model>` identity; transport hosts and aggregator names are not
guessed as original providers. Only stable CortexFS provider/model names and
positive limits enter the normalized cache.

The host cache is bounded, versioned data with this shape:

```json
{
  "schema": "cortexfs.model-limits/v1",
  "models": {
    "openai/gpt-5.6": 272000
  }
}
```

The cache is atomically replaced only after a complete successful online
response has been parsed and validated. A timeout, network error, invalid or
oversized response, empty validated result, or failed durable write preserves
the last valid cache unchanged. A missing, malformed, oversized, wrong-schema,
or unsafe cache supplies no limit. Catalog cache content contains no provider
credentials and is backend state, not a new `/ctx` namespace.

Model fallback is part of the single global `model/route` file, not a hidden
per-model control file. This keeps transport routing and model failover
observable in one route ABI. A model fallback rule uses the following form:

```text
model-fallback(openai/gpt-5.6) -> openai/gpt-5.6-sol, local/backup
```

When the selected model is unavailable or fails before producing a successful
answer, the runtime tries fallback models in order. Each candidate still uses
the normal provider registry, secret lookup, and `/ctx/model/route` egress
rules. The separate `fallback: direct` line remains the transport default and
must not be confused with `model-fallback(...)`.

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
    "scopes": ["model.read", "offline_access"],
    "device": {
      "request_url": "https://auth.example.com/device/code",
      "token_url": "https://auth.example.com/device/token",
      "verification_uri": "https://auth.example.com/device"
    }
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

## Provider Authentication Framework

Provider JSON may advertise more than one authentication method without
coupling a model to a provider-specific login command:

```json
{
  "base_url": "https://api.example.com/v1",
  "auth": [
    {"type": "api_key", "slot": "default"},
    {"type": "oauth", "flow": "authorization_code", "slot": "subscription"}
  ],
  "oauth": {
    "client_id": "cortexfs-example",
    "auth_url": "https://auth.example.com/authorize",
    "token_url": "https://auth.example.com/token",
    "redirect_uri": "http://127.0.0.1:8765/callback",
    "scopes": ["model.read", "offline_access"],
    "device": {
      "request_url": "https://auth.example.com/device/code",
      "token_url": "https://auth.example.com/device/token",
      "verification_uri": "https://auth.example.com/device"
    }
  }
}
```

`type` is `api_key` or `oauth`; OAuth `flow` is `authorization_code` or
`device_code`. `slot` is a logical credential slot and is not a keychain
account name. When `auth` is absent, CortexFS retains the compatibility
defaults of an API-key `default` slot plus an authorization-code OAuth method
when the legacy `oauth` block is present. Invalid slots or an OAuth method
without OAuth metadata fail closed during provider snapshot loading.

Adapters implement one provider-neutral boundary (`id`, supported methods,
authorization URL, login, device challenge, refresh, persistence, and model
listing) and return the normalized credential shape. The host can inject the
HTTP transport, clock, challenge notifier, and sleep callback for deterministic
tests; Agents never receive that transport or provider-native response types.
The built-in registry provides concrete OpenAI/Codex and Anthropic/Claude
adapters, plus a GitHub Copilot adapter when the host supplies its OAuth app
metadata. Claude and Copilot client registrations remain host configuration;
no provider client id is compiled into the Agent path.

```json
{
  "type": "oauth",
  "provider": "example",
  "access_token": "…",
  "refresh_token": "…",
  "expires_at": 123456789,
  "scopes": ["model.read"]
}
```

API-key credentials use `type: "api_key"`, `provider`, and `key`. These are
in-memory adapter envelopes only. Raw credentials never enter `/ctx`, model
objects, `.d/` controls, or model history; the existing root-owned secret
store remains the persistence boundary. Inspect the declared methods with:

```text
ctx provider auth methods PROVIDER
```

The command prints `method<TAB>flow<TAB>slot` and never prints secret material.
Model listing remains provider-neutral and feeds the existing model projection
and bounded host caches; it does not create an `/ctx/identity` namespace. The
existing hardened host discovery request is issued through the selected
adapter's model transport and parser, so provider-specific model envelopes do
not leak into the model ABI.
`device_code` is part of the shared declaration grammar. An OAuth `device`
block supplies standard device-code endpoints for host-configured adapters.
The built-in GitHub Copilot adapter supplies its documented defaults when that
block is omitted; `api.githubcopilot.com` also maps to the stable
`github-copilot` provider name when no explicit host name is supplied. Adapters
implement the standard device challenge, bounded
polling, and normalized credential persistence. The CLI prints the
verification URI and user code but never stores the device code in `/ctx`.

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
`/ctx/model/openai/gpt-5.6`. It does not create `/ctx/model/codex` or a second
provider namespace.

The Google preset uses Gemini's OpenAI-compatible endpoint. The Anthropic
preset uses `anthropic.messages`, so the runner sends `POST /v1/messages` with
the required Anthropic version header.

## One-Shot Exec

`/ctx/model/<provider>/<model>` is a read-only executable object. Reading it returns
CortexFS metadata text for that model. Executing it performs one-shot
inference through CortexFS/Rust runtime code or a provider adapter; model
objects must not be shell-script implementations.

The first metadata keys mirror common model-listing fields:

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
echo "hello" | /ctx/model/openai/gpt-5.6
echo '{"messages":[{"role":"user","content":"hello"}]}' | /ctx/model/openai/gpt-5.6
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
image_input
image_output
audio_input
audio_output
video_input
video_output
pdf_input
pdf_output
attachment
temperature
interleaved
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

`attachment` means the model accepts file attachments; it does not grant file
access. `temperature` means the adapter can expose temperature control, and
`interleaved` means reasoning content can be interleaved with normal output.
`tool_call_syntax` only means the model event stream may contain
tool-call-shaped events. It does not mean the model can execute tools. It
grants no tool permission.

## Canonical Event Stream

Model and agent streams use these event types:

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

`usage` requires `input_tokens` and `output_tokens`. When reported by a
provider, optional `cached_tokens` and `cache_write_tokens` record cache reads
and writes without changing those totals.

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
