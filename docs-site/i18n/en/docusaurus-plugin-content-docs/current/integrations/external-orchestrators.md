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
`ext/chat/room/888888`.


If an orchestrator needs run, step, or workflow state, store it in request JSON, thread metadata, audit context, or the orchestrator's own state store. Do not ask CortexFS to add project-specific top-level directories, or second execution abstractions such as `chan/`, `job/`, or `hook/`.
