---
title: Top-level Tree
---

# Top-level Tree

当前 ABI 顶层目录：

```text
/
  status
  cap/
  format/
  provider/
  model/
  home/
  group/
  shared/
  ext/
  space/
  agent/
  cluster/
  mcp/
  skill/
  tool/
  memory/
  vector/
  db/
  audit/
  control/
```

## Meanings

```text
status        全局状态
cap/          全局能力列表
format/       API 协议格式
provider/     后端提供商和账号实例
model/        全局模型索引
home/         类 /home 的用户入口
group/        本机组入口
shared/       共享项目/协作入口
ext/          外部平台入口
space/        策略视图
agent/        agent 定义、运行时和协作入口
cluster/      agent/worker 集群
mcp/          MCP server、tools、resources、prompts
skill/        skill registry 和 skill 内容投影
tool/         Cortex 原生工具和外部工具投影
memory/       全局记忆和索引入口
vector/       向量数据库后端
db/           PostgreSQL/SQLite 等结构化后端
audit/        全局审计视图
control/      全局控制节点
```
