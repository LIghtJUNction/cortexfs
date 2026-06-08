---
title: 外部编排器
---

# 外部编排器

Workflow engines, bot runtimes, desktop tools, and local applications should use CortexFS through discovery files and generic submission rules.

## 集成映射

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

## 不要写死 Fixture

不要写死 `home/1000`、`agent/helper`、`ext/chat/room/888888` 这类示例路径。正式集成应通过 `count`、`list`、`status`、`route`、`model` 等小文件发现对象。

## 不要新增项目专属根目录

外部编排器如果需要表达自己的 run、step、workflow，应写进请求 JSON、thread metadata 或 audit subject/agent context。不要要求 CortexFS 增加项目专属顶层目录。
