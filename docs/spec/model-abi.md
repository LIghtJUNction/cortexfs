# Model ABI

There is only one model ABI:

```text
/ctx/model/<name>       one-shot inference executable
/ctx/model/<name>.sock  optional CortexFS session socket
/ctx/model/<name>.d/    control files
```

Bottom-layer AI API formats do not enter the ABI. OpenAI Responses, Anthropic
Messages, Gemini GenerateContent, OpenAI-compatible chat, local runtimes, and
aggregator-specific request formats are Rig or provider-adapter details.
Bottom-layer stateful/stateless behavior does not enter the ABI. Rig adapts
provider connections, API compatibility, and streaming into the canonical
CortexFS request and JSONL event stream.

Example:

```text
/ctx/model/
  qwen
  qwen.sock
  qwen.d/
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
driver   internal driver name; first choice is rig
cap      capability list, one per line
default  default parameters, KEY=VALUE, one per line
session  none or socket
status   dynamic status
log      short call log or pointer to log location
```

## One-Shot Exec

`/ctx/model/<name>` is always one-shot inference:

```bash
/ctx/model/qwen "hello"
echo "hello" | /ctx/model/qwen
echo '{"messages":[{"role":"user","content":"hello"}]}' | /ctx/model/qwen
```

Semantics:

```text
one invocation
no durable session mutation
stdout is the canonical JSONL event stream
exit code is the process-level summary
```

Even if the underlying provider has native state, `/ctx/model/<name>` behaves
as a stateless single call. Scripts must be predictable.

## Model Socket

`/ctx/model/<name>.sock` is the only multi-turn model entry. It uses the shared
JSONL socket protocol from [object-abi.md](object-abi.md).

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

`model/<name>.d/session` has only two stable values:

```text
none    no /ctx/model/<name>.sock
socket  /ctx/model/<name>.sock exists and supports CortexFS sessions
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
{"type":"start","run":"r1","model":"qwen"}
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

`model/<name>.d/native` may exist for diagnostics only:

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
