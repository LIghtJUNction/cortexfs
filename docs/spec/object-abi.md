# Object ABI

Every executable object follows the same triple:

```text
name        executable entry: stateless, one-shot, or task entry
name.sock   socket entry: stateful, multi-turn, streaming
name.d/     control directory: config, state, permissions, logs
```

If an object does not support stateful interaction, do not expose `name.sock`.
A socket that only reports errors is bad ABI.

Do not expand one object into `profile/`, `runtime/`, `policy/`, `control/`,
and other layered trees. If a small file can say it, put it in `.d/`.

## Names and Aliases

ABI names under `model`, `agent`, and `tool` are path components. They do not
parse `provider/model` slash semantics.

Object name syntax:

```text
[a-zA-Z0-9][a-zA-Z0-9._+-]{0,63}
```

Forbidden:

```text
/
NUL
empty string
.
..
control characters
newline
suffix .sock
suffix .d
```

Rules:

```text
the filename is the stable alias
native ids do not enter root names
provider id, API format, and base URL do not enter root names
native ids may live in .d/id, .d/driver, or another control file
short user aliases use symlinks
```

Example:

```text
/ctx/model/qwen
/ctx/model/qwen.d/id = openrouter/qwen/qwen3-235b-a22b

/ctx/home/1000/model/coder -> /ctx/model/qwen
```

Alias resolution:

```text
symlink means symlink
readlink decides object identity
there is no alias.d override semantics
one path is not half alias and half real object
```

If `coder` needs its own default parameters, create a real object instead of a
symlink overlay:

```text
/ctx/model/coder
/ctx/model/coder.d/id
/ctx/model/coder.d/default
```

## Exec Protocol

`model/<name>`, `agent/<name>`, and `tool/<name>` are executable files. They
must accept argv or stdin input:

```bash
/ctx/model/qwen "hello"
echo "hello" | /ctx/model/qwen
echo '{"messages":[{"role":"user","content":"hello"}]}' | /ctx/model/qwen

/ctx/agent/coder "fix this project"
echo '{"task":"fix tests"}' | /ctx/agent/coder

/ctx/tool/fs.read '{"path":"README.md"}'
echo '{"path":"README.md"}' | /ctx/tool/fs.read
```

stdout should be JSONL:

```jsonl
{"type":"start","run":"r1","model":"qwen"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"done","run":"r1","status":"ok"}
```

Human-readable output can exist as compatibility mode. Machine callers should
prefer JSONL.

Exit codes:

```text
0   success
1   generic error
2   bad arguments or bad input format
13  permission denied, maps to EACCES
69  service unavailable, object exists but runtime is unavailable
70  internal error
```

If stdout has already started emitting JSONL, errors should continue as JSONL
error frames. The exit code is only the process-level summary.

TTY rules:

```text
model/<name>  with no args on a TTY may enter a simple REPL, but is not required to
agent/<name>  with no args on a TTY must enter an interactive socket session
tool/<name>   with no args on a TTY should print short usage or read stdin, not start a long session
```

## Socket Protocol

`model/<name>.sock` and `agent/<name>.sock` are Unix domain sockets. The
protocol is JSONL.

Requests:

```jsonl
{"op":"send","id":"msg-1","session":"default","cwd":"/work","input":"hello"}
{"op":"resume","session":"default","after":"event-id"}
{"op":"cancel","id":"run-id"}
{"op":"ping"}
```

Responses:

```jsonl
{"type":"start","id":"event-id","run":"run-id","model":"qwen"}
{"type":"delta","id":"event-id","run":"run-id","text":"..."}
{"type":"message","id":"event-id","run":"run-id","role":"assistant","content":[{"type":"text","text":"..."}]}
{"type":"error","run":"run-id","code":"EACCES","message":"permission denied"}
{"type":"done","run":"run-id","status":"ok"}
{"type":"pong"}
```

Socket lifecycle:

```text
missing        object does not support stateful mode, or service is not started
ECONNREFUSED   object declares a socket, but the process is unavailable
connected      requests and responses are JSONL frames
closed         private/shared sessions are not deleted
```

Hard socket rules:

```text
max frame size       v1 is 1 MiB; larger frames return EMSGSIZE
unknown fields       must be ignored
unknown op           returns EINVAL
after disconnect     private/shared sessions continue by default; temp sessions may be cancelled
client id retry      within one session, retry is idempotent and returns the original run id or final state
delta order          strictly increasing send order within one run
backpressure         blocking write is backpressure; implementation must not buffer forever in memory
```

When a client receives `SIGINT`, it should first send `cancel` to the socket.
Only the second interrupt, or a broken connection, should exit the client
process.

Error frames use stable errno names such as `EACCES`, `EINVAL`, `ENOENT`,
`EMSGSIZE`, and `EHOSTDOWN`. Clients must not parse natural language `message`.
