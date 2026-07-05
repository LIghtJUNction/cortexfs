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
默认参考树提供 `architect`、`coder`、`reviewer` 三个 agent：`architect` 负责规划和协调，
`coder` 是可修改 `/workspace` 源码的实现 agent，`reviewer` 负责独立审查。

启动默认编程 `coder`：

```bash
ctx bootstrap
ctx agent start coder --session default
ctx agent chat coder
```

改源码前可以先审计它看到的状态、工具和 prompt：

```bash
ctx agent status coder
ctx agent tools coder
ctx agent prompt coder
```

`ctx agent chat` 是人类聊天界面；`tsh` 是 agent terminal 里的工具 shell，不是同一个界面。
默认 `coder.d/system.md` 要求它先确认适用的项目规则和当前工作区状态，用
`fs.replace` 做小范围源码修改、运行可用的 format/static-check/lint/test 命令、检查 diff，
最后汇报改动文件和真实验证命令。完整路径可以用 `npm run bootstrap-coder:smoke` 复测。

父 agent 的 `context/plan.json` 可以把独立工作拆给 child agent；delegated 节点省略
`agent` 时默认使用 `worker`，省略 `session` 时默认使用当前父 session 名。单步推进计划使用：

```bash
ctx schedule status home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule advance home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule claim home/1000/agent/coder/session/default/context/plan.json work-123
ctx schedule result home/1000/agent/coder/session/default/context/plan.json work-123 done "实现完成"
ctx agent wait coder work-123 --session default
```

`status` 只读取 plan、child 状态表和 delegated worker 的 `agent/<name>`、`agent/<name>.d/model`、`life`、`parent`，
delegated backing agent 缺失时不会伪造 `main`/`owned` 默认值；
并输出 `node<TAB>kind<TAB>agent<TAB>child<TAB>session<TAB>model<TAB>life<TAB>role<TAB>child_parent<TAB>state`；`advance` 只物化 ready child handoff，`claim` 只把已物化的 child 从 `pending`
标记为 `active`，`result` 只把 child 的终态结果写回父 session 的
`context/child/<child>/`。命令输出会带上 parent ref，以及 child 的
`agent`、`session`、`model`、`life`、`role`、`child_parent`、`handoff.md`、`result.md`、`refs.jsonl` ABI 路径，父 agent 和 worker 不需要猜测交接文件位置、模型、角色或生命周期。
`agent wait` 是非阻塞的父进程式结果读取：child 还在 `pending`/`active` 时失败，进入
`done`/`error`/`cancelled` 后输出 `child<TAB>status<TAB>agent<TAB>session<TAB>model<TAB>life<TAB>role`
和 `result.md`，并分别以 0、1、130 作为进程退出码。
它们都不启动后台监听、轮询或第二套提交入口。
供应商密钥不写进 model 文件、`.d/` 控制目录或进程环境变量。provider adapter 从
root-owned CortexFS system secret store 直接读取 API key；用户不需要在 provider JSON
里手写环境变量名。长期凭据写入：
`/var/lib/cortexfs/secrets/provider/<provider>/<slot>`，通过 CLI 管理：

```bash
printf '%s\n' 'your-secret' | sudo ctx provider secret set local
ctx provider secret status local
```

常见 provider 可以先安装文件化 preset：

```bash
ctx provider preset list
ctx provider preset show google
ctx provider preset install codex
ctx provider preset install openai
ctx provider preset install anthropic
ctx provider preset install google
```

规范名称是 `openai`、`anthropic`、`google`。`codex` 是 `openai` preset 的别名；
`gemini` 是 `google` preset 的别名。安装 `codex` 后模型仍投影在规范路径
`/ctx/model/openai/<model>` 下，不新增 `/ctx/model/codex` 命名空间。

模型代理不做成 agent，也不写进 provider JSON。全局唯一路由表是：

```text
/ctx/model/route
```

这个文件同时决定 transport 和 key slot。多个 provider、多个模型、同一个 provider 的
多个 key，都通过这一张表分配：

```text
group(proxy) -> http(http://127.0.0.1:8080/v1), key(office)
group(local-socket) -> unix(/run/user/1000/cortexfs/proxy/openai.sock), key(local)

dip(198.51.100.45) -> direct
# dip(203.0.113.43) -> JP
domain(bestproxy.com) -> proxy
pname(NetworkManager, systemd-resolved, dnsmasq) -> must_direct
dip(geoip:private) -> direct
dip(geoip:cn) -> direct
domain(geosite:cn) -> direct
model(embedding-*) -> local-socket
fallback: proxy
```

`key(office)` 表示同一个 provider 的另一个凭据槽，对应 system secret store 的
`/var/lib/cortexfs/secrets/provider/<provider>/office`。不写 `key(...)` 就用
`default` 槽。

本地聚合 API 或 IP 地址 endpoint 必须显式配置稳定 provider 名，不要让地址字面量成为
`/ctx/model/<provider>` 路径：

```json
{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
```

对应模型路径是 `/ctx/model/local/gpt-5.4-mini`。密钥写入
`service=cortexfs:local account=default`；不需要在 provider JSON 里写明文密钥。

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

`ctx agent new` 优先调用 `/ctx/tool/agent.create`；如果该 tool 不存在，host 侧
`ctx` 会创建标准 `agent/<name>.d/*` 控制文件和 `home/<uid>/agent/<name>/`
skeleton。`ctx agent start` 直接启动显式 runtime；terminal socket 可达后会把
`agent/<name>.d/status` 写为 `ready`，并向 `agent/<name>.d/log` 追加
`agent.start` 事件，启动输出会显示 `model`、`life` 和 `role`。`ctx agent stop` 优先调用 `/ctx/tool/agent.stop`，如果该 tool
不存在则把 `agent/<name>.d/status` 写为 `dead`、清空 `pid`，并追加 `agent.stop`
事件。`ctx agent status` 和 `ctx agent ps` 只读取普通 `agent/<name>.d/*` 控制文件；
`agent status` 第一行仍是状态值，后续显示 `model=...`、`life=...`、`role=...`、`parent=...`、
`children=...`、`pid=...` 和 `ppid=...`，并继续显示 `uid=...`、`gid=...`、`groups=...`、`root=...`、`cwd=...`
这些 Linux 身份和路径字段；`parent=...` 使用和 `agent ps` 相同的规范化 parent ref，包含可选
`session`/`run`；`children=...` 只统计 effective 状态不是 `dead` 的直接 child，
记录了 stale 数字 pid 的 `ready`/`busy` child 会和 `ctx agent ps` 一样被排除；非默认模型会在进程树里显示为 `model=...`，非 `owned`
生命周期会显示为 `life=...`，worker-role agent 会显示为 `role=worker`。`ctx agent env NAME` 打印 `ctx agent start` 派生出的沙箱环境，便于检查 worker 实际获得的
`CTX_AGENT`、`CTX_AGENT_ROLE`、`CTX_AGENT_MODEL`、`CTX_AGENT_LIFE`、`CTX_AGENT_ROOT_PATH/CWD`、`CTX_AGENT_UID/GID/GROUPS`、`CTX_PATH`、`HOME` 等变量。`ctx agent children NAME` 从父 session 的
child 表读取任务状态，并同时显示 backing worker 的 `parent_session`、`parent_run`、`model`、
`life`、`role`、`status`、`ppid` 和 `pid`，方便按父进程视角检查 worker。

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

`tsh` 是 CortexFS tool shell，不是 host shell。standalone human `tsh` 解析命令的顺序是：

```text
1. CTX_HOME/.tshrc 里的 CTX_PATH=...
2. 进程环境 CTX_PATH
3. 默认 /ctx/tool:/ctx/home/<uid>/tool
```

agent terminal 里的 `tsh` 使用 agent runtime 按 policy、mount、uid/gid 生成的进程
`CTX_PATH`，不会让用户 `.tshrc` 覆盖这条授权路径。

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

`agent.sh coder` 会通过 `ctx agent chat coder --session default` 进入 agent 聊天界面；`ctx agent repl` 只是兼容别名。带 prompt
参数时由脚本转发到 `ctx agent send coder --session default ...` 发送一条消息。需要旁观 agent terminal 时使用 `agent.sh --watch coder`；需要进入
terminal、看到 `ctxterm -> tsh` 时，才使用 `agent.sh --attach coder`。
`agent.sh` 不保存私有聊天数据库。

## 安装后多轮 smoke

安装后的最小验证应该走现有 session ABI，而不是另建测试入口：

```bash
ctx bootstrap
ctx agent start coder --session default --cwd /workspace
ctx agent send coder --session default "第一轮：读取当前任务"
ctx agent send coder --session default "第二轮：基于上一轮继续"
ctx agent history coder --session default
ctx agent output coder --session default
```

这条路径同时验证了 `agent/<agent>.sock`、`messages.jsonl`、`latest.md`、当前
session 选择和 prompt history 注入。多轮对话的持久事实仍在
`/ctx/home/<uid>/agent/<agent>/session/<session>/messages.jsonl`；`ctx agent prompt`
只用于检查将要发送给模型的渲染结果，不替代真实 socket 对话。

需要把独立实现任务交给 spark worker 时，父 agent 先用 `ctx schedule advance` 物化
handoff，然后把输出里的 `model=`、`life=`、`role=`、`parent=`、`child_parent=`、`plan=`、`handoff=`、`result=`、`refs=`
交给 worker。worker 只用同一套 `ctx schedule claim/result` 写回结果；不要新增队列、轮询器或第二套
coordination 文件。

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
