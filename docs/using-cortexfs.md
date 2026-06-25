---
id: using-cortexfs
title: 日常使用
sidebar_label: 日常使用
---

# 日常使用

CortexFS 的日常体验应该像 Unix：先发现对象，再读取状态，需要执行时就执行文件或连
socket。

## 找到可用对象

```bash
ctx ls model
ctx ls agent
ctx ls tool
```

常见对象形状：

```text
/ctx/model/main
/ctx/model/debug/echo
/ctx/agent/coder
/ctx/agent/coder.sock
/ctx/tool/fs.read
```

同名文件负责执行，同名 `.sock` 负责有状态 JSONL 交互，同名 `.d/` 目录保存小型控制
文件。

## 直接调用模型

调试时可以先用 echo 模型：

```bash
/ctx/model/debug/echo "hello cortex"
echo "summarize this file" | /ctx/model/main
```

切换默认模型时改 `/ctx/model/main` alias，而不是在根目录新增 provider 专用入口。
供应商密钥不写进 model 文件或 `.d/` 控制目录；provider adapter 会按环境变量、系统
keychain、未配置的顺序解析。

## 管理 agent

`ctx agent` 是当前 ABI 的薄客户端。创建、启动和停止 agent 仍然走普通 tool 或文件
ABI，不引入新的 workflow 入口：

```bash
ctx agent new reviewer --model openai/gpt-4o --tool fs.read
ctx agent new reviewer --label reviewer_t --shared project-a:read --mount /work /work ro
ctx agent start reviewer --session default
ctx agent status reviewer
ctx agent ps
ctx agent stop reviewer
```

如果 `/ctx/tool/agent.create`、`agent.start` 或 `agent.stop` 不存在，对应生命周期命令会
以 service unavailable 失败。`ctx agent status` 和 `ctx agent ps` 只读取普通
`agent/<name>.d/*` 控制文件。

## 观察和接入 agent 终端

`ctx agent start` 默认把调用者当前目录挂载到 sandbox 内的 `/workspace`，并从
`/workspace` 启动 `ctxterm -> tsh`。如果调用者当前目录包含 `.git`，`.git` 会被
额外覆盖挂载到 `/workspace/.git` 只读。agent 的 `HOME` 是沙箱自己的
`/home/agent`，不会把 shell 配置和缓存写进项目目录：

```bash
ctx agent start coder --session default
ctx agent watch coder --session default
ctx agent attach coder --session default
```

底层终端 socket 位于：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

FUSE 路径可以是指向 `/run/user/<uid>/cortexfs/terminal/.../main.sock` 的 symlink；旧安装
也可能指向 `/run/cortexfs/terminal/.../main.sock`。`watch` 只读；`attach` 会把你的
stdin 接入终端。

需要精确控制 sandbox 时：

```bash
ctx agent start coder --session review \
  --no-default-workspace \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

## 使用 tool shell

`tsh` 是 CortexFS tool shell，不是 host shell。它解析命令的顺序是：

```text
1. 进程环境 CTX_PATH
2. CTX_HOME/.tshrc 里的 CTX_PATH=...
3. 默认 /ctx/tool:/ctx/home/<uid>/tool
```

`.tshrc` 是数据文件，不执行 shell 语法：

```text
CTX_PATH=/ctx/tool:/ctx/home/1000/tool
```

常用检查：

```bash
tsh --list
tsh fs.read '{"path":"README.md"}'
```

## 使用 agent.sh

仓库仍然包含 `agent.sh` 作为 shell 前端：

```bash
install -m 0755 agent.sh/agent.sh ~/.local/bin/agent.sh
agent.sh --help
agent.sh coder
agent.sh coder "summarize this repository"
agent.sh --chat coder
agent.sh --session default coder "inspect the failing test"
agent.sh --resume coder
```

`agent.sh coder` 会连接 agent terminal；如果 terminal 尚未运行，会先执行
`ctx agent start coder`，因此正常会看到 `ctxterm -> tsh` 的会话。带 prompt 参数时
才发送一条 agent socket 消息。需要聊天式 socket REPL 时使用 `agent.sh --chat coder`。
`agent.sh` 不保存私有聊天数据库。

## 自定义 agent

agent 的用户可编辑系统提示词在：

```text
/ctx/agent/<agent>.d/system.md
```

例如：

```bash
ctx cat agent/coder.d/system.md
ctx set agent/coder.d/system.md "You are a careful Rust coding agent."
```

`system.md` 只定义 persona 和工作风格，不授予权限。agent 默认 native tool 仍只有
`tsh`；其他工具必须通过 `tsh` 发现、加载、pin 和调用。实际权限仍由
`agent/<agent>.d/policy`、`path`、`mount`、Linux uid/gid 和 mode bits 决定。

## 使用共享空间

共享空间是普通文件目录，适合放项目材料、任务输入和 agent 之间要交换的结果：

```bash
ctx path shared project-a
cd "$(ctx path shared project-a)"
```

agent 是否能读写某个共享目录，由它的 view、mount、policy、Linux uid/gid 和 mode
bits 决定。

## 查看历史

```bash
ctx agent history coder
ctx agent output coder
```

不传 `--session` 时，`ctx agent history` 和 `ctx agent output` 会先使用
`session/index/current`，不存在时退回 `default`。因此查看当前/latest session 不需要
单独的 `latest` 子命令。

底层历史在：

```text
/ctx/home/<uid>/agent/<agent>/session/
```

原始 history 是持久事实；context 是可重建工作集。压缩上下文不能销毁原始消息。
