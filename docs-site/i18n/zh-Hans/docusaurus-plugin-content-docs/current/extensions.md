---
id: extensions
title: 单文件扩展
sidebar_label: 单文件扩展
---

# 单文件扩展

CortexFS 遵循与 Pi 相同的反框架扩展规则：在稳定边缘（包、可执行文件、skills、
modules、channel 适配器）增加行为，而不引入第二套根 ABI 或常驻插件守护进程。
这些边缘在产品中的位置见 [architecture.md](architecture.md)（「扩展点」）。
本页是最短编写路径。

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

先在不写入 backing tree 的情况下校验整包，再安装：

```bash
ctx install --check ./review-kit
ctx install ./review-kit
```

分发预构建包时，请在每个成员的 `run` 旁写入所分发文件的精确小写摘要，并要求所有成员都携带摘要：

```toml
[[tools]]
name = "git.summary"
run = "bin/git-summary"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

```bash
ctx install --check --require-hashes ./review-kit
```

请将示例摘要替换为 `sha256sum` 的输出。即使没有 `--require-hashes`，任何已声明的 `sha256` 也都会被检查；该参数还会在任一工具或 Agent 省略摘要时拒绝整包。

`ctx install --check` 会发现 `cortexfs.toml`、哈希每个可执行文件、渲染并检查所有严格对象 manifest，然后在选择或写入 backing tree 前退出。普通 `ctx install` 会重复这些检查，再通过现有原子对象安装器发布每个对象。摘要匹配只能把描述符绑定到可执行文件字节，不能认证发布者或注册表。请通过经过认证的可信通道取得描述符和预期摘要；`cortexfs.package/v1` 尚未定义签名。

若挂载树有固定来源代际，请加 `--source PATH`，若工具只对当前用户可见请加 `--tier user`：

```bash
ctx install ./review-kit --source /var/lib/cortexfs/storage/current
```

Agent Unix 身份由宿主授权决定，不属于包 metadata。安装器根据有效用户和补充组推导身份；包作者不能选择 uid、gid 或特权组。

`run` 项就是扩展入口。工具实现 Tool SDK，代理实现 Agent SDK；两者都只是普通可执行文件，因此 Rust、Shell 或其他主机语言都能实现。SDK agent 从标准输入读取一个运行时 Envelope，向外输出 JSONL 事件；它可能产出一次工具调用，由宿主执行权限校验并将观察结果回写给下一步。这就是自定义执行循环，不依赖常驻插件守护进程。

官方默认实现仍可覆盖，控制文件与 SDK helper 如下：

```text
loop=chat|react|coding|planner|research   内置行为提示（默认 chat）
loop=<name> + loop.d/<name>               自定义 loop 驱动可执行文件
compact.strategy=truncate|summarize|<name>  历史重建策略（默认 truncate）
compact.d/<name>                          自定义压缩可执行文件
invoke.strategy=default|cli|sdk|<name>    工具调用面（默认 default）
invoke.d/<name>                           自定义工具 invoke 可执行文件
adapter=<family>|<name>                   channel 适配器族或自定义名
adapter.d/<name>                          自定义 channel socket 驱动
Agent SDK BuiltinLoop                     在自定义二进制内解释 CTX_AGENT_LOOP
Tool SDK InvokeMode                       读取 CTX_TOOL_MODE / CTX_AUTHORIZED_OBJECT
Channel SDK DriverLaunchConfig            读取 CORTEXFS_CHANNEL_* / CTX_CHANNEL_* 环境变量
```

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
