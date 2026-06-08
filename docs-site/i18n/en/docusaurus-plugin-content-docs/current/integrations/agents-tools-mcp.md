---
title: Agents, Tools, MCP
---

# Agents, Tools, MCP

CortexFS models agents, tools, and MCP as auditable filesystem objects.

Agent:

```text
agent/<id>/
  profile.md
  policy/
  tool/
  skill/
  memory/
  thread/
  inbox/
  outbox/
  runtime/
```

Tool:

```text
tool/<tool-id>/
  input_schema.json
  output_schema.json
  invoke/
    inbox/
    outbox/
```

MCP:

```text
mcp/server/<id>/
mcp/tool/<server>.<tool>/
mcp/resource/
mcp/prompt/
mcp/session/
```

MCP calls must go through Cortex policy and audit.
