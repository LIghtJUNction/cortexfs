---
id: using-cortexfs
title: 日常使用
sidebar_label: 日常使用
---

# 日常使用

CortexFS 的日常使用体验应当像 Unix 一样：先发现对象、读取状态，再在需要执行任务时运行文件或连接套接字。

## 查找可用对象

```bash
ctx ls model
ctx ls agent
ctx ls tool
```

常见对象形态：

```text
/ctx/model/main
/ctx/model/debug/echo
/ctx/agent/coder
/ctx/agent/coder.sock
/ctx/tool/fs.read
```

命名文件用于执行工作，匹配的 `.sock` 处理有状态的 JSONL 交互，匹配的 `.d/` 目录保存较小的控制文件。

在某些部署中，`/ctx/agent/<name>.sock` 是指向用户运行时套接字的所有者授权符号链接；在某些系统部署中它也可能是一个直接的套接字节点。两种形式都应当视为有效实现，按当前挂载暴露的形态使用 `nc -U` 或 `readlink` 探测。

## 直接调用模型

调试时先用回显模型：

```bash
/ctx/model/debug/echo "hello cortex"
echo "summarize this file" | /ctx/model/main
```

当你想更换默认模型时，修改 `/ctx/model/main` 的别名即可。请通过修改别名，而不是新增 provider 特定的根目录入口来完成。

参考树定义了 `architect`、`coder`、`reviewer`、`worker`：

`architect` 是根规划与协调代理；`coder`、`reviewer` 和 `worker` 以 `agent:architect` 为父项。

用以下命令启动并检查参考源：

```bash
ctx bootstrap
ctx bootstrap --check
ctx bootstrap --dry-run
```

`ctx bootstrap` 仅在以下任一项发生变化时才写入 `bin/cortexfs.bootstrap.json`：架构、树版本、受管代理列表或需要刷新的迁移条目。已淘汰的 `base` 与 `executor` 对象仍会被报告并保留，供人工复核，因为旧安装没有所有权清单和完整控制树完整性证明。一次成功的 `bootstrap` 会让下一次 `--check` 结果干净。

默认的 `coder.d/system.md` 会把 `coder` 视为父级整合方：独立实现工作应表现为 `context/plan.json` 中的委托 `react` 节点；若委托节点省略 `agent`，则默认 `worker`，若省略 `session`，则使用当前父会话名。用如下命令推进计划：

```bash
ctx schedule status home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule advance home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule claim home/1000/agent/coder/session/default/context/plan.json work-123
ctx schedule result home/1000/agent/coder/session/default/context/plan.json work-123 done "implemented"
ctx agent wait coder work-123 --session default
```

`status` 读取计划、子节点表和委托 worker 的 `agent/<name>`、`agent/<name>.d/model`、`life`。当委托 backing agent 缺失时，它不会伪造 `main`/`owned` 默认值，随后打印
`node<TAB>kind<TAB>agent<TAB>child<TAB>session<TAB>model<TAB>life<TAB>state`。
`advance` 会材料化已就绪的子交接，`claim` 会将已材料化的子任务从 `pending` 变为 `active`，`result` 会把终态结果写入 `context/child/<child>/`。命令输出除了父引用外，还会携带子任务的 `agent`、`session`、`model`、`life`、`handoff.md`、`result.md` 和 `refs.jsonl` 的 ABI 路径，以免父节点或 worker 需要猜测协调状态。`agent wait` 是类似 `waitpid` 的非阻塞读取：当子任务处于 `pending` 或 `active` 时返回失败；当子任务进入 `done`、`error` 或 `cancelled` 时，打印
`child<TAB>status<TAB>agent<TAB>session<TAB>model<TAB>life`，随后打印 `result.md`。这些命令不会启动后台监听、轮询循环或第二套提交入口。

Provider 的密钥不会写入模型文件或 `.d/` 控制目录。提供者适配器会优先从提供者环境变量候选项解析 API Key（若设置），其次读取 CortexFS 系统密钥存储
`/var/lib/cortexfs/secrets/provider/<provider>/<slot>`。若缺少所需凭据，模型状态为 `unconfigured`。

先为常见提供者安装文件化预设。`list` 会打印预设名、认证方式和目标文件：

```bash
ctx provider preset list
ctx provider preset show google
ctx provider preset install openrouter
ctx provider preset install deepseek
ctx provider preset install compatible --name local --base-url http://127.0.0.1:8317/v1 --model custom-model
ctx provider preset install openai
ctx provider preset install anthropic
ctx provider preset install google
```

规范提供者名仍是 `openai`、`anthropic` 与 `google`。`gemini` 是 `google` 的别名。聚合与地区 OpenAI 兼容预设会写入显式 `name`，使 `/ctx/model/<provider>` 保持稳定对象名。`compatible` 用于任意兼容端点，不是某个本地运行时的特殊分支。

模型代理不属于任何 agent，也不会写入 provider JSON。唯一的全局路由表是：

```text
/ctx/model/route
```

该文件同时决定传输方式与密钥槽。多个提供者、多个模型与一个提供者的多个密钥都通过该表路由：

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

`key(office)` 表示同一提供者的另一个凭据槽，并对应
`/var/lib/cortexfs/secrets/provider/<provider>/office`。若省略 `key(...)`，CortexFS 使用 `default` 槽。

## 管理代理

`ctx agent` 是当前 ABI 的轻量客户端。创建、启动与停止代理仍通过常规工具或文件 ABI 完成，它不会新增工作流入口：

```bash
ctx agent new reviewer --model openai/gpt-5.6 --tool fs.read
ctx agent new reviewer --label reviewer_t --shared project-a:read --mount /work /work ro
ctx agent new --from .cortexfs/agents/reviewer/agent.yaml
ctx agent apply reviewer --from reviewer
ctx agent start reviewer --session default
ctx agent status reviewer
ctx agent ps
ctx agent stop reviewer
```

主机侧的 `agent.yaml`（以及 `agent.yml` 和 `agent.json`）是编排输入。`ctx agent new --from` 与 `ctx agent apply --from` 会校验并将其实体化为 `agent/<name>.d/*`；运行时权限仍仅来自离散控制文件。`--from NAME` 会搜索 `.cortexfs/agents` 与 `~/.config/cortexfs/agents`。

也可使用 Eve 项目作为同样的输入源，无需安装 Node 运行时：

```bash
ctx agent new --from ./my-eve-agent
ctx agent apply reviewer --from ./my-eve-agent
```

导入器只会读取 `agent/instructions.md`（静态）和 `agent/agent.ts` 中的字面量 `model: "provider/model"`。它会记录发现到的 Eve 工具、技能、频道、子代理与 schedule 并写入代理描述，但不会执行 TypeScript，不会暴露密钥，不会启动 HTTP 通道，也不会创建监听器。除非安装并通过策略允许相应的 CortexFS 工具，否则 Eve 能力仅作为源数据保留，确保在 Git/进程刷新边界处可复现。

```yaml
schema: cortexfs.agent.profile/v1
name: reviewer
description: code review agent
instructions: Review diffs carefully.
model: openai/gpt-5.6
tools: [fs.read]
parent: agent:architect
```

`ctx agent new` 优先使用 `/ctx/tool/agent.create`；若该工具缺失，则由主机端 `ctx` 生成标准的 `agent/<name>.d/*` 控制文件和 `home/<uid>/agent/<name>/` 骨架。`ctx agent start` 启动显式运行时；当终端套接字可达后，它会写入 `agent/<name>.d/status=ready`，并向 `agent/<name>.d/log` 追加 `agent.start` 事件。`ctx agent stop` 优先调用 `/ctx/tool/agent.stop`；若缺失则写入 `agent/<name>.d/status=dead`、清空 `pid`，并追加 `agent.stop` 事件。`ctx agent status` 与 `ctx agent ps` 仅读取普通的 `agent/<name>.d/*` 控制文件。`agent status` 先保留首行为状态值，再输出 `model=...`、`life=...`、`parent=...`、`children=...`、`pid=...`、`uid=...`、`gid=...`、`groups=...`、`root=...`、`cwd=...`。`children=...` 会统计状态不是 `dead` 的直接子项；`ready` 或 `busy` 且 pid 已过期的子项会被排除，规则与 `ctx agent ps` 一致。
非默认模型与非 `owned` 生命周期会在 `ctx agent ps` 中可见。`ctx agent env NAME` 会打印由 `ctx agent start` 派生的沙箱环境，`ctx agent children NAME` 显示父视角的子任务状态，以及底层 worker 的 `parent_session`、`model`、`life`、`status`、`pid`。

AGFS 风格的服务组合通过现有文件 ABI 完成：使用 `shared/<space>/data` 存放持久值，使用文档化的
`shared/<space>/queue/{inbox,pending,lease,claimed,done,failed}` 重命名协议处理任务，并用受控的 `fs.read`、`fs.list`、`fs.stat`、`fs.write` 工具进行检查与变更。系统里不存在常驻插件 daemon、轮询 worker 或心跳命名空间；事实提交和普通 session/status 文件仍是事实源。

## 提交图片与其他文件

对于图片、PDF、音频、归档或其他二进制内容，请提交路径引用，不要把字节直接塞进提示词。CortexFS 保留文件本身在工作区或共享空间，并让代理可见；对话中只需描述任务和路径。

在当前目录启动代理时，该目录会默认挂载到 `/workspace`：

```bash
ctx agent start coder --session default
ctx send coder "Analyze /workspace/assets/screenshot.png and summarize UI issues"
```

需要更严格可见性时，使用显式挂载：

```bash
ctx agent start coder --session image-review \
  --no-default-workspace \
  --mount "$PWD/assets" /input ro \
  --mount "$PWD/docs" /docs ro \
  --cwd /docs

ctx send coder --session image-review "Inspect /input/screenshot.png and use /docs/DESIGN.md"
```

当多个代理或会话共享同一材料时，使用共享空间：

```bash
mkdir -p "$(ctx path shared project-a)/input"
cp screenshot.png "$(ctx path shared project-a)/input/"
ctx agent new reviewer --shared project-a:read
ctx send reviewer "Inspect /ctx/shared/project-a/input/screenshot.png"
```

这样可避免大文件进入消息历史。上下文只记录路径、摘要和引用；读取图片字节、提取文本、渲染缩略图或调用视觉模型应通过可见工具按需执行。

## 观察并连接终端

`ctx agent start` 默认会将调用者当前目录以只读方式挂载到沙箱内的 `/workspace`，随后从 `/workspace` 启动 `ctxterm -> tsh`。若调用目录包含 `.git`，额外以只读方式覆盖挂载到 `/workspace/.git`。代理的 `HOME` 是沙箱自身的 `/home/agent`，因此 shell 配置和缓存不会写入项目目录：

```bash
ctx agent start coder --session default
ctx agent watch coder --session default
ctx agent attach coder --session default
```

终端套接字位于：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

FUSE 可见路径统一指向 root 所有的 `/run/cortexfs/terminal/broker.sock`。`ctx` 会认证 broker，并请求指定的 Agent/会话；不会回退到旧用户级终端 socket。`watch` 是只读的；`attach` 会将你的标准输入连接到终端。

在需要时显式控制沙箱：

```bash
ctx agent start coder --session review \
  --no-default-workspace \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

## 使用工具壳

`tsh` 是 CortexFS 的工具壳，不是宿主 shell。独立的人类会话中的 `tsh` 会按以下顺序解析：

```text
1. CTX_HOME/.tshrc 中的 CTX_PATH=...
2. 进程 CTX_PATH
3. 默认 CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool
```

在代理终端内，`tsh` 使用代理运行时基于策略、挂载和 uid/gid 推导出的进程 `CTX_PATH`。主机用户的 `.tshrc` 不会覆盖该授权路径。

`.tshrc` 是数据文件，不是 shell 语法：

```text
CTX_PATH=/ctx/tool:/ctx/home/1000/tool
```

---

可用的检查命令：

```bash
tsh --list
tsh which fs.read
tsh help fs.read
```

直接调用工具时，优先在代理终端通过 `tsh` 发起，这样 CortexFS 可统一应用代理策略、挂载、uid/gid 与 `CTX_PATH`。

具有 `agent.update` 授权的代理可自迭代：工具会通过主机校验的运行能力套接字，原子替换调用代理自己的 `system.md` 或 `prompt.template.md`，新提示在下一次运行中生效。其他代理控制仍由主机所有，具体契约见 `docs/spec/tool-policy-abi.md`。

## 使用 agent.sh

仓库仍保留 `agent.sh` 作为 shell 前端：

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

`agent.sh coder` 通过 `ctx agent chat coder --session default` 打开聊天界面；带入 prompt 参数时会转发一条消息到 `ctx agent send coder --session default`。使用 `agent.sh --watch coder` 观察代理终端，`agent.sh --attach coder` 仅在需要进入 `ctxterm -> tsh` 时使用。`agent.sh` 不会保有私有聊天数据库。

## 已安装环境中的多轮 smoke

安装后，应使用现有会话 ABI 做最小化多轮 smoke，而非新增测试入口：

```bash
ctx bootstrap
ctx agent start coder --session default --cwd /workspace
ctx agent send coder --session default "round one: read the current task"
ctx agent send coder --session default "round two: continue from the previous turn"
ctx agent history coder --session default
ctx agent output coder --session default
```

该路径会校验 `agent/<agent>.sock`、`messages.jsonl`、`latest.md`、会话选择与 prompt 历史注入。会话的持久事实保留在
`/ctx/home/<uid>/agent/<agent>/session/<session>/messages.jsonl`；`ctx agent prompt` 仅用于检查将要发送给模型的 prompt，不应替代实时套接字会话。

将独立实现工作交给 worker 时，父代理先用 `ctx schedule advance` 材料化交接，再提供子任务发出的 `model=`、`life=`、`plan=`、`handoff=`、`result=`、`refs=` 字段。worker 再通过同一条 `ctx schedule claim/result` 路径回写，不要新增队列、轮询器或第二套协调文件。

## 自定义代理

用户可编辑的系统提示位于：

```text
/ctx/agent/<agent>.d/system.md
/ctx/agent/<agent>.d/prompt.template.md
```

示例：

```bash
ctx cat agent/coder.d/system.md
ctx set agent/coder.d/system.md "You are a careful Rust coding agent."
ctx cat agent/coder.d/prompt.template.md
ctx agent prompt coder
```

`system.md` 只定义人设与工作风格，`prompt.template.md` 定义这些内容如何与规则、技能元数据、工具注入、消息历史和运行时契约拼接，构成模型可见的第一条系统消息。模板变量包括 `{{agent}}`、`{{current_time_unix}}`、`{{agent_instructions}}`、`{{rules}}`、`{{skills}}`、`{{tool_injection}}`、`{{history_messages}}`、`{{runtime_contract}}`。

`ctx agent prompt <agent>` 会打印 CortexFS 当前可渲染的运行时系统提示，可用于检查模板、代理指令、可见 `AGENTS.md` 规则、受限技能元数据与运行时契约。真实模型调用时，工具注入和历史上下文仍由运行时按上下文窗口填充。

技能列表仅注入 `name`、`description` 与 `SKILL.md` 路径。完整 `SKILL.md` 内容仅在选择技能后再读取。技能元数据最多使用 2% 的上下文窗口；若窗口大小未知则硬上限为 8000 字符。超出配额时优先缩短描述，仍超出时会省略部分技能并在 prompt 中给出警告。

代理运行时会在该代理私有 session 目录中落一份“可用”快照（文本内容与 `{{rules}}` / `{{skills}}` 一致；快照写入不会阻塞运行）：

```bash
cat /ctx/home/$(id -u)/agent/coder/session/default/AGENTS.md
cat /ctx/home/$(id -u)/agent/coder/session/default/SKILLS.md
```

- `AGENTS.md`：生效规则（全局 + 项目层合并）
- `SKILLS.md`：仅技能元数据（`name`、`description`、`path`），不包含完整 `SKILL.md` 本体

这些 prompt 文件不授予任何权限。默认原生工具仍是 `tsh`；其他工具必须被发现、加载、Pin，并通过 `tsh` 调用。有效权限仍由 `agent/<agent>.d/policy`、`path`、`mount`、Linux uid/gid 与 mode bits 决定。

## 使用共享空间

共享空间是普通文件目录。用于项目素材、任务输入与各代理之间交换的结果：

```bash
ctx path shared project-a
cd "$(ctx path shared project-a)"
```

是否可读写共享目录由代理视图、挂载、策略、Linux uid/gid 与 mode bits 决定。

## 查看历史

```bash
ctx agent history coder
ctx agent output coder
ctx agent trajectory coder
```

不加 `--session` 时，命令首先读取 `session/index/current`，再回退到 `default`，因此查看当前最新会话不需要独立的 `latest` 子命令。

底层历史位于：

```text
/ctx/home/<uid>/agent/<agent>/session/
```

历史是耐久事实；上下文是可重建工作集。上下文压缩不能破坏原始消息。`ctx agent trajectory` 会校验并打印所选会话 `messages.jsonl` 与 `events.jsonl` 的 ATIF 投影；工具调用、观测与用量仍按运行/调用 id 关联，命令不会创建第二套历史存储。
