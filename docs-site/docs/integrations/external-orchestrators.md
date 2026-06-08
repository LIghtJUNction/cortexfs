---
title: External Orchestrators
---

# External Orchestrators

Workflow engines, bot runtimes, desktop tools, and local applications should use CortexFS through discovery files and generic submission rules.

## Integration Map

```text
能力            接入方式
路由观察        read home/<uid>/route/<format>/{provider,model,reason}
API 提交        rename to home/<uid>/api/<format>/inbox/<id>.req.json
Thread 提交     rename to home/<uid>/thread/<id>/inbox/<id>.req.json
Tool 调用       rename to tool/<tool-id>/invoke/inbox/<id>.req.json
MCP 调用        rename to mcp/tool/<server>.<tool>/invoke/inbox/<id>.req.json
记忆写入        rename to home/<uid>/memory/<layer>/inbox/<id>.req.json
审计读取        read audit/events.jsonl
训练导出        read home/<uid>/export/*.jsonl
```

## Do Not Hard-code Fixtures

当前实现中的 `home/1000`、`agent/helper`、`ext/qq/group/888888` 是 MVP 测试投影。正式集成应通过 `count`、`list`、`status`、`route`、`model` 等小文件发现对象。

## No Project-specific Root

外部编排器如果需要表达自己的 run、step、workflow，应写进请求 JSON、thread metadata 或 audit subject/agent context。不要要求 CortexFS 增加项目专属顶层目录。
