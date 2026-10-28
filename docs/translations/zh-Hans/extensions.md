---
id: extensions
title: 单文件扩展
sidebar_label: 单文件扩展
---

# 单文件扩展

CortexFS 遵循反框架扩展原则：在稳定边缘增加包、可执行文件、skills、modules 与 channel adapter，不引入第二套 root ABI 或常驻 plugin daemon。本页描述现有兼容面的最短 authoring 路径；新 hosted Agent CLI 集成优先使用薄 process launch profile。

最短形式仍是一个 package directory：

```text
review-kit/
├── cortexfs.toml
└── bin/
    ├── review-agent
    └── git-summary
```

```toml
schema = "cortexfs.package/v1"
name = "review-kit"
version = "0.1.0"

[[tools]]
name = "git.summary"
run = "bin/git-summary"
description = "Summarize the current Git worktree"
schema = { type = "object" }

[[agents]]
name = "kit_reviewer"
run = "bin/review-agent"
abi = "sdk-envelope-v1"
model = "main"
tools = ["git.summary"]
instructions = "Review changes, use the tool when useful, and cite evidence."
parent = "agent:architect"
```

上述 Agent SDK / `sdk-envelope-v1` 是旧自研 runtime 的兼容面。新的 Codex CLI、Claude Code、Pi、Antigravity 等集成不应继续在这里增加 provider/model/session/tool-loop 行为。

先校验，再显式安装：

```bash
ctx install --check ./review-kit
ctx install ./review-kit
```

预构建包可在 `run` 旁声明精确小写 SHA-256，并用 `--require-hashes` 要求所有成员都绑定 digest。Digest 只能把描述符绑定到 bytes，不认证 publisher；descriptor 与预期 digest 应从可信通道获得。

若挂载树有固定 source generation，可用：

```bash
ctx install ./review-kit --source /var/lib/cortexfs/storage/current
```

Agent Unix identity 是宿主权限，不属于 package metadata。包作者不能选择 uid、gid 或特权组。

Tool SDK 与旧 Agent SDK 的 `run` 都是普通 executable。Legacy SDK agent 从 stdin 读 envelope、输出 JSONL event，并可 yield tool call 让宿主执行 capability check；这是兼容行为，不是新 hosted CLI 的目标 runtime。

旧兼容 control 仍可能包括：

```text
loop=chat|react|coding|planner|research
loop.d/<name>
compact.strategy / compact.d/<name>
invoke.strategy / invoke.d/<name>
adapter / adapter.d/<name>
Agent SDK BuiltinLoop
Tool SDK InvokeMode
Channel SDK DriverLaunchConfig
```

迁移期间不要再为这些 control 增加新的 Agent-loop 功能。

Topology 仍可通过 `parent` edge 表达；package 只是 authoring input，安装后仍落入既有 `agent/<name>.d/*`、`tool/<name>.d/*`、session 文件与 socket，不新增 `/ctx` namespace。

开发/配置激活严格以 Git commit 为边界。必须先提交 package/config 变化；之后 process restart 只是重新消费已经提交 generation 的普通生命周期动作，绝不能让未提交 package/config 变成权威状态。`ctx install` 不会启动 watcher、polling loop 或后台 plugin service。