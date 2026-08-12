---
id: extensions
title: 单文件扩展
sidebar_label: 单文件扩展
---

# 单文件扩展

添加新能力最短的方式是一个包目录。把程序逻辑放在普通可执行文件里，把装配逻辑放在一个 `cortexfs.toml` 文件里：

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
model = "main"
tools = ["git.summary"]
instructions = "Review changes, use the tool when useful, and cite evidence."
parent = "agent:architect"
```

只需一条命令安装：

```bash
ctx install ./review-kit
```

`ctx install` 会发现 `cortexfs.toml`、哈希每个可执行文件、校验整包清单，并通过现有原子对象安装器发布每个对象。若挂载树有固定来源代际，请加 `--source PATH`，若对象只对当前用户可见请加 `--tier user`：

```bash
ctx install ./review-kit --source /var/lib/cortexfs/storage/current
```

当有特权安装器面向其他用户时，请在包内显式声明运行身份，不要继承安装者的 root 凭据：

```toml
[[agents]]
name = "kit_reviewer"
run = "bin/review-agent"

[agents.identity]
uid = 1000
gid = 1000
groups = [1000]
```

`run` 项就是扩展入口。工具实现 Tool SDK，代理实现 Agent SDK；两者都只是普通可执行文件，因此 Rust、Shell 或其他主机语言都能实现。SDK agent 从标准输入读取一个运行时 Envelope，向外输出 JSONL 事件；它可能产出一次工具调用，由宿主执行权限校验并将观察结果回写给下一步。这就是自定义执行循环，不依赖常驻插件守护进程。

拓扑关系仅由 `parent` 指定。每个代理通过 `agent:NAME` 声明父节点（`session:` 和 `run:` 仍可选）：

```toml
[[agents]]
name = "planner"
run = "bin/planner"
parent = "agent:architect"

[[agents]]
name = "builder"
run = "bin/builder"
parent = "agent:planner"
```

包文件是编排输入，不会创建第二个 `/ctx` 命名空间。安装后的持久产物仍然只有
`agent/<name>.d/*`、`tool/<name>.d/*`、普通 session 文件和现有套接字。`ctx object install` 命令仍保留给需要完整清单控制的包作者；大多数用户不需要直接接触它。

刷新是显式行为：要么提交新包，要么重启消费该代际的进程。`ctx install` 不会启动文件监听、轮询循环或后台插件服务。
