---
title: External Orchestrators
---

# External Orchestrators

Workflow engines, bot runtimes, desktop tools, and local applications should use
CortexFS through discovery files and generic submission rules.

```text
route observation    read home/<uid>/route/<format>/{provider,model,reason}
API submission       rename to home/<uid>/api/<format>/inbox/<id>.req.json
thread submission    rename to home/<uid>/thread/<id>/inbox/<id>.req.json
tool invocation      rename to tool/<tool-id>/invoke/inbox/<id>.req.json
MCP invocation       rename to mcp/tool/<server>.<tool>/invoke/inbox/<id>.req.json
memory write         rename to home/<uid>/memory/<layer>/inbox/<id>.req.json
audit read           read audit/events.jsonl
training export      read home/<uid>/export/*.jsonl
```

Do not hard-code MVP fixtures such as `home/1000`, `agent/helper`, or
`ext/qq/group/888888`.
