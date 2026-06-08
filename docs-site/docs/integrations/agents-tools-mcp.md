---
title: 智能体、工具和 MCP
---

# 智能体、工具和 MCP

CortexFS 把 agent、tool 和 MCP 都建模成可审计文件系统对象。

## 智能体

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

Agent 是带 profile、policy、tool、skill、memory 和 thread 的执行主体。

## 工具

```text
tool/<tool-id>/
  name
  description
  input_schema.json
  output_schema.json
  invoke/
    inbox/
    outbox/
```

Tool loop 是 thread/agent 下的 append-only 执行链，必须记录 tool call、permission、result 和错误。

## MCP

```text
mcp/server/<id>/
mcp/tool/<server>.<tool>/
mcp/resource/
mcp/prompt/
mcp/session/
```

MCP 调用必须走 Cortex policy 和 audit。MCP server 不能绕过 provider、tool、secret、space 权限。
