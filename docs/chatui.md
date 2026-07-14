# Chat UI ABI

`ctxchat` is the reference terminal UI. It is separate from the runtime: it
reads documented `/ctx` files and exchanges newline-delimited JSON with
`/ctx/agent/<agent>.sock`. UI settings and themes belong in plain files or
environment variables; the runtime does not require a UI database.

Durable history lives below
`/ctx/home/<uid>/agent/<agent>/session/<session>/`: `messages.jsonl` contains
messages, `events.jsonl` contains events, `latest.md` is a convenience view,
and `state` is the terminal state.

Minimal Bash clients:

```bash
printf '%s\n' '{"op":"send","id":"ui-1","session":"default","input":"hello"}' |
  socat - UNIX-CONNECT:/ctx/agent/coder.sock
printf '%s\n' '{"op":"tsh","id":"ui-tool-1","session":"default","args":["load","bash"]}' |
  socat - UNIX-CONNECT:/ctx/agent/coder.sock
```

Minimal Python client:

```python
import json, socket
s = socket.socket(socket.AF_UNIX)
s.connect("/ctx/agent/coder.sock")
s.sendall((json.dumps({"op": "send", "id": "py-1", "session": "default",
                       "input": "hello"}) + "\n").encode())
s.shutdown(socket.SHUT_WR)
for line in s.makefile():
    frame = json.loads(line)
    print(frame)
    if frame.get("type") == "done":
        break
```

`ctxchat` adds completion, multiline paste, `:tsh` commands, bounded `@path`
and `@history:N` references, and clipboard adapters. References are prompt
context only and never grant filesystem or tool authority.
