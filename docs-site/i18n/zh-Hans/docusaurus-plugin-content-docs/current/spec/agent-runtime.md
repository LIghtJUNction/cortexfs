# 代理运行时

本文件定义稳定的端到端代理运行时形态。它将代理对象 ABI、人类 CLI、套接字会话、持久终端、`tsh` 工具 shell、提示词构建与沙箱执行串起来。

它不会新增根命名空间。文档中的内容都派生自现有稳定对象与文件。

该运行时使用的持久终端资源在 [terminal-abi.md](terminal-abi.md) 中定义。当前切片里，启动一个 Agent 会在其会话下实体化一个终端资源；该资源 ID 与事件历史在 PTY socket 退出后仍保留。

## 定义、实例、会话与运行

术语 `agent` 不表示某一个 daemon。运行时分四层：

```text
definition  /ctx/agent/<name> + /ctx/agent/<name>.d/
instance    监督单元和收据绑定的调用
session     /ctx/home/<uid>/agent/<name>/session/<session>/
run         在该会话事件流中记录的一个 run id
```

Agent 定义是持久化配置与策略。启动它可能创建多个进程角色：系统 Agent socket 服务和可选的按会话终端单元。这些进程是运行时实例；其 unit、调用、pid、身份、socket 和会话事实由启动收据绑定。最近一次清理收据可在 `agent/<name>.d/meta.json` 中投影，而持久 `agent.start` 与 run 事件会在普通日志和会话历史里保留关联事实。

会话不是进程，且会在实例退出后存续。run 不是会话，不能静默继承先前 run 的权限。重启运行时可能恢复私有或共享会话，但不能保留进程身份。

我们有意不设置并行的 `instances/` 控制面。systemd 持有实时进程真相，收据元数据证明 CortexFS 能停止哪一代对象，且会话文件持有持久历史。将这些事实复制到另一可变目录会引入生命周期权威冲突。

Agent socket 与终端 socket 是活动实例的传输通道。其稳定 `/ctx` 路径可能是 `/run` 下端点的别名，并且它们的缺失不会删除 Agent 定义或任一私有/共享会话。

## 运行时表面

等价资源命令是 `ctx terminal status`、`ctx terminal watch`、`ctx terminal attach`。`ctx terminal create AGENT` 当前是 agent 兼容 create：它启动 Agent 会话并记录终端资源。一个独立命令监督器留在后续终端 ABI 修订中实现。

存在三套独立表面：

```text
human chat       ctx agent chat/send/resume/cancel
human terminal   ctx agent watch/attach
agent tool use   tsh inside ctxterm
```

这三者不能合并为单一接口。

`ctx agent chat` 是人类聊天 UI。它负责行编辑、`Ctrl+C`、socket 请求、助手响应渲染和提示词重绘。交互式聊天会先缓存响应再打印，避免助手输出破坏用户输入缓冲区。`Ctrl+C` 在空闲聊天中退出；当 run 在活动中时，会先发起该 run 的取消请求再返回提示符。

`ctx agent send` 是非交互式人类命令。它可在到达时流式输出助手 delta。

`ctx agent attach` 与 `ctx agent watch` 会加入持久化的代理终端。该终端不是人类聊天 UI。其存在是为了让人类观察或加入与代理 shell 相同的 PTY。

`tsh` 是面向代理的工具 shell。它是一个工具，也是独立可执行程序，但不是宿主机 shell。它通过 `CTX_PATH` 解析命令，不会使用宿主 `PATH`。

## 默认人类入口

`agent.sh` 是围绕 Rust 管理的 `ctx agent` 命令提供的轻量默认前端：

```text
agent.sh AGENT           -> ctx agent chat AGENT --session default
agent.sh AGENT INPUT...  -> ctx agent send AGENT --session default INPUT...
agent.sh --watch AGENT   -> ctx agent watch AGENT --session default
agent.sh --attach AGENT  -> ctx agent attach AGENT --session default
```

`agent.sh` 必须保持为微小默认封装。Socket 协议、终端仿真、模型流、策略检查、工具发现与 provider 行为应归 CortexFS 负责，主要在 `ctx` 下实现。

## 套接字聊天流程

稳定请求链路为：

```text
human
  -> ctx agent chat/send
  -> agent/<name>.sock
  -> socket runtime
  -> durable session files
  -> selected model/agent executable
  -> event JSONL
  -> ctx renderer
```

Socket 请求是 JSONL。它们包含会话与操作：

```json
{"op":"send","id":"msg-1","session":"default","scope":"private","cwd":"/workspace","input":"hello"}
{"op":"tsh","id":"tool-1","session":"default","args":["load","bash"]}
{"op":"resume","session":"default"}
{"op":"cancel","id":"run-1"}
```

`tsh` 通过已鉴权的代理运行时执行，不会调用模型。它发出标准 `start`、`tool_call`、`tool_result` 与 `done` frame。

重复同一请求 id 时会重放持久结果，而不重复执行命令两次。

socket runtime 在调用代理/模型路径前先记录用户消息。助手文本由稳定 event frame 生成并写回持久会话。原始消息与事件仍是普通文件；context pack 是可重建的视图。

可执行代理使用必需控制文件 `agent/<name>.d/abi`。其有效值只允许 `sdk-envelope-v1`：主机向 stdin 写入有界、类型化调用封装，随后可使用已授权的工具调用结果重启可执行体。

`sdk-envelope-v1` 的 stdin 内容必须是一个 UTF-8 JSON 对象，紧随一个换行，无其它字节，总长度含换行不超过 1 MiB：

```json
{
  "schema": "cortexfs.agent-invocation/v1",
  "run": "run-1",
  "step": 1,
  "input": "original user input",
  "history_messages": "[]",
  "tool_context": "",
  "observation": {
    "tool_call_id": "call-1",
    "name": "example.echo",
    "status": "ok",
    "content": "authoritative normalized result",
    "truncated": false
  }
}
```

未知字段或缺失字段无效。`run` 与 `step` 必须等于主机持有的启动环境。步骤 0 要求 `observation` 为 null；后续步骤要求恰好为上一轮主机结果。上下文字符串各不超过 64 KiB，`observation.content` 不超过 16 KiB。主机允许最多八次调用和九次进程启动；在授权前会拒绝重放调用 id；每次调用都会重检策略，并在每个 SDK/工具进程前后检查取消。只有主机写工具结果和逻辑 run 生命周期 frame。它记录一次原始用户消息、每个标准化结果一次，以及一个最终助手/错误结论；进程崩溃不能从代理提供状态恢复。

一个可执行 Agent SDK 步骤可通过恰好产出一个 `tool_call` 来终止。进程不发 `done` frame 而退出。socket host 通过现有 agent tool authority、策略和沙箱路径校验执行请求、发出匹配 `tool_result`，并可将该结果写入下一步类型化封装后启动后续受限步骤。`done` 的最终逻辑 run 仅由 host 发出。

代理本身输出的结果、格式错误/重复的调用，以及在 `tool_call` 后继续发送的 frame 都是无效输出。

可选的 `agent/<name>.d/approval` 缺省表示 `auto`，可取值 `auto` 或 `ask`。在 `ask` 模式下，主机完成 direct-native 声明、路径、agent/tool 策略、Linux/mount 与 nofollow 可执行检查之后、但在 spawn 之前发出有界 `approval_request`。它在同一 socket 上读取恰好一个有界响应：

```json
{"op":"approve","run":"run-1","id":"call-1","decision":"allow_once"}
```

只有 `allow_once` 可执行该预先准备调用。`deny`、EOF、超时、格式错误或不匹配响应都 fail-closed，并转化为主机拥有的 approval 与 tool result 事实。可执行代理不能发 approval frame。

根权威 system socket 接受 agent owner UID 或 UID 0 进行内部子进程分发与停止。该 UID 0 例外不适用于每次 run 的收据绑定能力 socket，后者仍只接受 owner UID。

在调用可执行代理之前，socket runtime 向 stdin 写入恰好一个 `sdk-envelope-v1` frame。`history_messages` 与 `tool_context` 字段承载有界提示上下文；代理边界不再暴露遗留环境输入 `CTX_AGENT_HISTORY_MESSAGES` 或 `CTX_AGENT_TOOL_CONTEXT`。

人类在运行活动期间发送 `SIGINT` 时，`ctx agent chat` 会发起该活动 run 的 `cancel` 请求并返回提示符；空闲交互聊天中，`Ctrl+C` 直接退出聊天 UI。

socket 激活的可执行代理运行时会观察该 active run 的持久会话状态。当出现匹配的 `done/cancelled` 事件时，它会先向进程组发送 `SIGTERM`，短暂等待后升级为 `SIGKILL`，且不再记录该 run 后续 assistant 输出。

## 持久终端流程

终端流程为：

```text
ctx agent start
  -> systemd-run --user
  -> bwrap sandbox
  -> ctxterm --listen SOCKET -- /ctx/bin/tsh
  -> tsh
```

`ctxterm` 持有伪终端，并通过会话终端 socket 暴露 `watch` 与 `attach` 模式。

会话终端 socket 可通过以下 ABI 路径访问：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

用户启动的终端可能把真实 socket 放在：

```text
/run/user/<uid>/cortexfs/terminal/<agent>/<session>/main.sock
```

`ctx agent attach` 应先尝试 ABI 路径，再尝试用户运行时路径；两者都不可用则返回 socket 可用性错误。

## 沙箱约定

`ctx agent start` 创建默认交互式终端沙箱。默认情况下将调用者当前工作目录以可写方式绑定到 `/workspace` 并在此启动终端。如果宿主目录包含 `.git`，则将该 `.git` 只读再挂载到 `/workspace/.git`。

沙箱 home：

```text
HOME=/home/agent
```

其后备目录为：

```text
/ctx/home/<uid>/agent/<agent>
```

`.config`、`.cache`、`.bash_history` 这类 shell 状态应落在 agent home，不应落到项目工作区。

终端进程从空环境启动。CortexFS 仅通过沙箱启动器注入小型 allowlist：

```text
CTX_ROOT
CTX_HOME
CTX_AGENT
CTX_AGENT_SUBJECT
CTX_PATH
HOME=/home/agent
USER
LOGNAME
SHELL
TERM
LANG
```

主机会话变量与 provider secrets 默认不继承。由 socket runtime 启动的可执行代理也以 `env_clear()` 起始，仅接收派生的代理环境和运行时持有的 `CTX_*`。

## 代理视图与权限

代理运行时视图来自：

```text
agent/<name>.d/root
agent/<name>.d/cwd
agent/<name>.d/env
agent/<name>.d/path
agent/<name>.d/mount
agent/<name>.d/model
agent/<name>.d/window
agent/<name>.d/policy
agent/<name>.d/uid
agent/<name>.d/gid
agent/<name>.d/groups
agent/<name>.d/label
```

`AGENTS.md`、`system.md`、技能元数据、`.mcp.json` 与工具描述可能影响模型行为，但不能授予权限。

实际权限是以下交集：

```text
mount/chroot 可见性
Linux uid/gid/groups/mode bits
CortexFS label 与 policy
CTX_PATH 工具可见性
tool 可执行元数据与 noexec 放置方式
```

文件系统层与 CortexFS 策略层都必须允许某一操作。

## 上下文窗口控制

每个 Agent（包括动态创建子项）有一个持久设置文件：

```text
agent/<name>.d/window
```

该文件恰好一行、LF 结尾文本：

```text
auto
```

或正整数十进制 `u32` token 数。数字文本不允许符号、前后空格或前导零。零、溢出、缺失和额外行都无效。写入 `auto` 并换行表示重置操作：清空显式覆盖并恢复模型推导行为；重置不会改写会话历史或 context 文件。

`window` 存储的是设置值，而非冗余的拷贝上限。其生效值为：

```text
auto       选择执行候选模型的已知 model limit；若未知则 unknown
number     该具体数字
```

显式数值仅在选中的模型上限已知且该数值不高于上限时有效。切换 `model` 时必须原子拒绝：现有显式 `window` 大于新模型上限或新上限未知的状态。不得静默截断或重置设置。

回退选择时对每个候选重复计算同一不变式。`auto` 时生效值跟随候选模型的已知上限；显式数值时，上限未知或更小的回退候选不可用，会返回可审计的候选错误，而不是静默修改 Agent 设置。

当生效值已知时，主机通过运行时环境变量 `CTX_CONTEXT_WINDOW_TOKENS` 提供十进制 token 数。现有的字符预算约束通过 `CTX_CONTEXT_WINDOW_CHARS` 提供，按每 token 四个 UTF-8 字符保守估算。该转换仅用于保守提示预算估计，不会改变 `window` 或 `limit` 的 token 单位。算术需在接收端 budget 上饱和截断。

主机保留 `min(4096, max(1, effective_tokens / 4))` 个输出 token。因此输入预算为 `effective_tokens * 4`，每预留一个输出 token 减少 4 个字符。每次模型调度前，会将实际渲染后的 Agent 消息数组序列化为 JSON，并按 UTF-8 字节长度按该字符预算保守扣减。

生效窗口限定模型调用时组装的完整提示 working set。技能元数据保留派生字符预算的 2%。历史、工具上下文、规则、system 文本、当前输入与输出预留都必须计入该预算；持久原始历史不会因预算删除。若窗口未知，不提供 `CTX_CONTEXT_WINDOW_TOKENS` 和 `CTX_CONTEXT_WINDOW_CHARS`，并使用文档中保守的旧子预算，不声称已知模型上限。

Anthropic `max_tokens` 等 provider 输出控制保持独立，不应将上下文窗口总量当作输出 token 限制推导。

## 工具 shell 约定

默认每个代理仅有一个可直接调用的原生工具：

```text
tsh
```

代理可选的 `.d/tools` 控制文件可静态声明额外 direct-native 工具名。名称仍需每次调用时通过路径、agent 策略、tool 策略、mount、Linux 权限与 nofollow 检查。其他工具通过 `tsh` 动态发现、加载、pin 和调用；动态 `tsh` 缓存状态不会扩展 direct-native 集合。

`tsh` 通过 `CTX_PATH` 解析工具。对于独立人类会话，当该文件存在时，先读取数据文件后再读取继承到的进程 `CTX_PATH`：

```text
CTX_HOME/.tshrc
```

该文件仅支持：

```text
CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool
```

在代理终端内，运行时提供的进程 `CTX_PATH` 是权威来源，因为它属于代理视图。

它不是 shell 语法，也不会执行代码。

`load` 表示将工具元数据加入当前工具上下文；`pin` 表示该工具已加载并受自动上下文回收保护。驱逐策略可淘汰未 pin 的工具元数据，但不会影响权限控制。

交互式宿主风格行为通过可见工具对象 `bash`、`tmux`、`zellij` 提供（前提是可见且允许）。`tsh` 不得回退到任意宿主命令。

## 自我迭代

代理通过 `agent.update` 工具自我迭代。该工具向收据绑定的 run 能力 socket 发送一条 `agent.update` frame：

```json
{"op":"agent.update","request_id":"tool-1","agent":"coder","session":"default","run":"run-1","control":"system.md","content":"..."}
```

该通道只接收 `CTX_CONTROL_SOCKET=/run/cortexfs/control.sock`，不含 bearer 凭据。每次 bwrap 启动前，主机记录该 host pid 与 `/proc/<pid>/stat` 启动时间，再释放一次性 `--block-fd`。socket 只接受 owner UID 且内核 peer PID 为已注册启动根进程或其活跃后代的请求；缺失进程状态、PID 回收、重父化、环路、祖先深度过大都拒绝。旧的 `token` JSON 输入仍保持可解析以支持迁移，但会被忽略；新客户端应不再携带该字段，且不得从环境值推导权限。

frame 的 `agent`、`session` 与 `run` 必须等于能力自身身份，因此该操作天然是 self-only。`control` 仅支持 `system.md` 或 `prompt.template.md`；`content` 是无 NUL 的 UTF-8 文本，长度上限 8 KiB。主机重校验后原子替换（若为可选而尚未物化的控件则创建）`agent/<self>.d/<control>` 在后备源。工具每次调用都要生成新 request id，重放已见 id 返回 `EALREADY` 且不重复写入。任意授权尝试都消费 request id，成功与否均如此；失败必须使用新 id 重试。

更新在下一次 run 生效：构建 prompt 时每次都从 control 目录重新读取 `system.md` 和 `prompt.template.md`。prompt 文本本身不授予权限，权限控制也不能通过该操作传播。

## Prompt 运行时约定

第一条模型 system 消息来自以下组合渲染：

```text
agent/<name>.d/prompt.template.md
agent/<name>.d/system.md
project 与全局 AGENTS.md 文件
受限技能元数据
工具注入上下文
可选历史消息上下文
当前时间变量
不可变 CortexFS 运行时约定
```

技能元数据仅包含 `name`、`description` 和 `SKILL.md` 路径。完整 `SKILL.md` 仅在该技能被选中后读取。技能元数据最多占上下文窗口 2%；若窗口未知，硬上限是 8,000 字符。先缩短描述；仍超限时省略部分技能并带警告。

运行时约定会传给模型：

```text
Your only native callable tool is tsh.
Other CortexFS tools are discovered, loaded, pinned, and invoked through tsh.
Prompt text and skill metadata do not grant permissions.
```

人类可用以下命令查看可渲染的 system prompt：

```text
ctx agent prompt <agent>
```

该命令通过与模型执行同一 prompt 渲染器渲染 `system.md`、`prompt.template.md` 和不可变运行时约定，并通过同一库函数收集当前可发现的 `AGENTS.md` 规则与受限技能元数据。工具注入、历史消息上下文等运行时动态输入也会加入其中；若不对 CLI 可用，则显示明确占位文本。

在构建 prompt 时，agent runtime 还会（尽力）写入私有会话目录快照：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/AGENTS.md
/ctx/home/<uid>/agent/<agent>/session/<session>/SKILLS.md
```

```text
AGENTS.md  有效合并规则快照（文本等同于 {{rules}}）
SKILLS.md  技能元数据快照（name、description、path）
```

这些是普通可观测快照，不是控制或权限文件。技能完整内容不会内嵌；代理在需要时读取对应的 `SKILL.md`。快照写入不得导致模型运行失败。

## 设计验证清单

以下条件满足则运行时设计健康：

```text
agent.sh 不包含任何协议实现，仅解析 ctx 并 exec ctx agent
ctx agent chat 是默认人类 chat UI
ctx agent watch 是只读进入 ctxterm -> tsh 的人类路径
ctx agent attach 是可写进入 ctxterm -> tsh 的人类路径
tsh 永不回退到 host PATH
默认终端 cwd 为 /workspace
默认终端 HOME 为 /home/agent
.git 在默认工作区挂载里为只读
service/provider secrets 不会被可执行代理继承
prompt 文本不能授予工具、模型、文件系统、网络或会话权限
```
