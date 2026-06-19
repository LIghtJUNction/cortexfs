---
title: Task Submission
---

# Task Submission

CortexFS does not expose `home/<uid>/job` and does not define an internal job DSL. A task is just a request file: external software puts its task spec in JSON and submits it through the generic inbox/outbox path; filesystem paths still use existing entries such as `api/`, `thread/`, `tool/`, `mcp/`, and `memory/`.

Unified rule:

```text
write tmp file
rename tmp -> <id>.req.json
read outbox/<id>.resp.json or outbox/<id>.error
read audit/events.jsonl
```

## Example

```bash
CTX_HOME="/ctx/home/$(id -u)"
api="$CTX_HOME/api/openai.chat"

cat > "$api/inbox/translate-001.tmp" <<'JSON'
{"messages":[{"role":"user","content":"Translate to zh-CN: hello world"}]}
JSON

mv "$api/inbox/translate-001.tmp" "$api/inbox/translate-001.req.json"
printf '1\n' > /ctx/control/drain
cat "$api/outbox/translate-001.resp.json"
```

If a workflow engine needs run IDs, step IDs, input sources, or retry policies, store those in the request JSON, thread metadata, or the engine's own state store. CortexFS only provides the generic provider, route, policy, queue, outbox, audit, and export plane.
