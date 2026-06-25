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

## 提交图片和其他文件

给 agent 图片、PDF、音频、压缩包或其他二进制材料时，推荐提交“路径引用”，不要把文件内容
塞进 prompt。CortexFS 的思路是：文件本体留在 agent 可见的 workspace 或 shared space，
对话里只描述任务和路径。

当前目录启动 agent 时会默认挂载为 `/workspace`：

```bash
ctx agent start coder --session default
ctx send coder "请分析 /workspace/assets/screenshot.png，总结界面问题"
```

需要显式控制可见目录时：

```bash
ctx agent start coder --session image-review \
  --no-default-workspace \
  --mount "$PWD/assets" /input ro \
  --mount "$PWD/docs" /docs ro \
  --cwd /docs

ctx send coder --session image-review "请查看 /input/screenshot.png，并参考 /docs/DESIGN.md"
```

需要让多个 agent 或多次会话共享同一批材料时，放到 shared space：

```bash
mkdir -p "$(ctx path shared project-a)/input"
cp screenshot.png "$(ctx path shared project-a)/input/"
ctx agent new reviewer --shared project-a:read
ctx send reviewer "请检查 /ctx/shared/project-a/input/screenshot.png"
```

这样做的好处是：大文件不进入消息历史；上下文里只记录路径、摘要和引用；真正读取图片、
抽取文本、生成缩略图或调用视觉模型，由可见 tool 在需要时完成。

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
tsh which fs.read
tsh help fs.read
```

直接执行 tool 时，推荐从 agent terminal 里使用 `tsh`，这样 CortexFS 可以同时应用
agent policy、挂载、uid/gid 和 `CTX_PATH`。

## 使用 agent.sh

仓库仍然包含 `agent.sh` 作为 shell 前端：

```bash
install -m 0755 agent.sh/agent.sh ~/.local/bin/agent.sh
agent.sh --help
agent.sh coder
agent.sh coder "summarize this repository"
agent.sh --chat coder
agent.sh --attach coder
agent.sh --watch coder
agent.sh --session default coder "inspect the failing test"
agent.sh --resume coder
```

`agent.sh coder` 会通过 `ctx agent-sh coder` 进入 agent 聊天 REPL；带 prompt
参数时由 `ctx agent-sh` 转发到 `ctx agent send` 发送一条消息。需要旁观 agent terminal 时使用 `agent.sh --watch coder`；需要进入
terminal、看到 `ctxterm -> tsh` 时，才使用 `agent.sh --attach coder`。
`agent.sh` 不保存私有聊天数据库。

## 自定义 agent

agent 的用户可编辑系统提示词在：

```text
/ctx/agent/<agent>.d/system.md
/ctx/agent/<agent>.d/prompt.template.md
```

例如：

```bash
ctx cat agent/coder.d/system.md
ctx set agent/coder.d/system.md "You are a careful Rust coding agent."
ctx cat agent/coder.d/prompt.template.md
ctx agent prompt coder
```

`system.md` 只定义 persona 和工作风格；`prompt.template.md` 决定它和规则、skill
元数据、工具注入内容、历史消息上下文、runtime contract 如何组成模型看到的第一条
system message。模板变量包括 `{{agent}}`、`{{current_time_unix}}`、
`{{agent_instructions}}`、`{{rules}}`、`{{skills}}`、`{{tool_injection}}`、
`{{history_messages}}`、`{{runtime_contract}}`。

`ctx agent prompt <agent>` 会打印 CortexFS 当前可渲染出的 runtime system prompt。
它用于检查模板、agent instruction、当前可发现的 AGENTS.md 规则、bounded skill 元数据
和 runtime contract 是否按预期组合；真实模型调用时，工具注入和历史上下文仍由运行时按
上下文窗口动态补齐。

Skill 列表只注入 `name`、`description`、`SKILL.md` 路径；完整 `SKILL.md` 只在选中
skill 后读取。Skill 元数据最多占上下文窗口 2%；上下文大小未知时硬上限为 8,000
字符。超限时先缩短 description，仍超限则省略部分 skill 并在 prompt 中给出警告。

这些 prompt 文件不授予权限。agent 默认 native tool 仍只有 `tsh`；其他工具必须通过
`tsh` 发现、加载、pin 和调用。实际权限仍由 `agent/<agent>.d/policy`、`path`、
`mount`、Linux uid/gid 和 mode bits 决定。

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
