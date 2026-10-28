# agent.sh

`agent.sh` 是一个针对 Rust 拥有的微小 Linux 默认前端
`ctx agent` 命令。这不是 CortexFS 运行时，而是一个套接字协议
实现、提供者 SDK、调度程序或私人聊天数据库。

它依赖于 Bash 和一个 `ctx` 二进制文件。它不使用 `nc`、`jq`、Python,
Node、npm、Cargo、云 SDK、提供商客户端、包管理器或直接
提供商 API。所有代理协议行为都保留在 `ctx` 内。

## 安装

在 `PATH` 的某个地方安装仓库副本：

```bash
install -m 0755 agent.sh/agent.sh ~/.local/bin/agent.sh
```

检查已安装的前端：

```bash
agent.sh --help
```

## 边界

`agent.sh` 是一个小型的默认包装器，而不是 ABI 阅读器。它解析 `ctx`
然后用常见的默认值如 `--session default` 执行 `ctx agent ...`。
下面的稳定路径是 `ctx` 读取和写入的 CortexFS 状态：

```text
/ctx/agent/<agent>.sock
/ctx/agent/<agent>.d/
/ctx/home/<uid>/agent/<agent>/session/
/ctx/tool
/ctx/home/<uid>/tool
/ctx/shared
```

在某些部署中，`/ctx/agent/<agent>.sock` 是一个由所有者授权的符号链接指向
一个用户运行时套接字，并且在某些部署中它 m
可能是一个直接的套接字节点。
在假设一种实现形式之前，先探查实时挂载。

`/ctx/tool` 是系统工具层级。`/ctx/home/<uid>/tool` 是用户自己的
工具层级，而不是系统工具默认符号链接副本的存放位置。一个真正的
代理运行时可能会看到这些层的经过过滤的内存中 FUSE 投影。

它不使用根命名空间，例如 `provider`、`format`、`cluster`、
`control`、`thread`、`workflow`、`mcp` 或 `skill`。
## 环境

```bash
export CTX_ROOT=/ctx
export CTX_HOME="$CTX_ROOT/home/$(id -u)"
export CTX_PATH="$CTX_ROOT/tool:$CTX_HOME/tool"
```

当这些变量未设置时，默认值是从相同的值派生的。
`CTX_PATH` 是一个源层级列表；策略、挂载点、uid/gid 和模式位
仍然决定特定代理可以执行什么。

## 使用方法

```bash
agent.sh executor
agent.sh executor "fix tests"
agent.sh --chat executor
agent.sh --attach executor
agent.sh --watch executor
agent.sh --session default executor
agent.sh --resume executor
agent.sh --history executor
agent.sh --pack executor
agent.sh --tools executor
agent.sh --children executor
agent.sh --cancel executor
agent.sh --status executor
agent.sh --raw executor "prompt"
```

在没有提示的情况下，`agent.sh AGENT` 通过打开代理聊天界面
`ctx agent chat AGENT --session default`。通过提示，它会调度到
`ctx agent send AGENT --session default`。

使用 `agent.sh --watch AGENT` 观察代理终端的只读。使用
`agent.sh --attach AGENT` 仅当你想加入终端并查看时
`ctxterm -> tsh`。

## 聊天和
终端

`ctxchat` 拥有行编辑、参考文献、剪贴板适配器、套接字请求，
并通过已记录的文件/套接字 ABI 进行响应渲染。`ctx agent
chat` 执行 `ctxchat`。

在聊天外壳中，`/workspace` 打印挂载的主机结账点
`/workspace`；`/status` 打印代理模型、生命周期、角色和工作区；
`/tools` 列出了可见的 CortexFS 工具。

`ctx agent send` 是非交互式路径，可能会传输助手增量数据
他们到达。

`ctx agent attach` 是一个不同的工作流程：它连接了持久代理 PTY。
那个 PTY 运行 `ctxterm -> tsh`；`tsh` 是面向代理的工具外壳，而不是
人类聊天界面。

`ctx` 使用的套接字请求格式是换行分隔的 JSON：

```json
{"op":"send","id":"ctx-...","session":"default","scope":"private","cwd":"/workspace","input":"fix tests"}
{"op":"tsh","id":"tool-...","session":"default","args":["load","bash"]}
{"op":"resume","session":"default"}
{"op":"cancel","id":"run-1"}
```

默认情况下，响应由 `ctx agent` 作为助手文本呈现。传递 `--raw`
打印原始 JSONL 事件。

## 会话

`agent.sh` 从不存储私人历史。它读取稳定的会话树：

```text
$CTX_HOME/agent/<agent>/session/index/current
$CTX_HOME/agent/<agent>/session/<session>/messages.jsonl
$CTX_HOME/agent/<agent>/session/<session>/events.jsonl
$CTX_HOME/agent/<agent>/session/<session>/latest.md
$CTX_HOME/agent/<agent>/session/<session>/context/
```

如果没有选择会话，`index/current`
在存在时使用，否则使用
会话名称是`default`。

使用 `ctx agent output <agent>` 打印最新的助手输出
选定的会话。省略 `--session` 遵循相同的 `index/current`，然后
`default` 规则。

## 工具与儿童

`--tools` 列出通过 `CTX_PATH` 发现的可执行文件，并
`agent/<agent>.d/path`。它不在本地决定政策。

`--children` 从以下位置读取子任务状态：

```text
$CTX_HOME/agent/<agent>/session/<session>/context/child/
```
