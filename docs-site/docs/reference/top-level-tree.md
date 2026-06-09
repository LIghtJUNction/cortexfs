---
title: 顶层目录
---

# 顶层目录

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

## 含义

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

不提供 `/ctx/chan`、`home/<uid>/job`、`home/<uid>/hook` 或 `workflow/`。中转站或账号实例属于 `provider/`；路由属于 `home/<uid>/route/`；外部任务和触发器直接写入通用 inbox 并通过 rename 提交。挂载树不是可扩展数据目录；对未声明 ABI 目录执行 `mkdir` 会返回 EROFS。
