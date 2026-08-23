# CortexFS 架构

规范 ABI 细节在 [spec/](spec/)。视觉规范位于
[DESIGN.md](DESIGN.md)（Google Labs DESIGN.md 风格）。本文档是工程设计入口：定义
CortexFS 是什么、状态放在哪里，以及哪些内容不能成为稳定根 ABI。

## 一页模型

```text
/ctx 是面向代理运行时的 FUSE 文件系统接口。
model 是纯推理文件。
agent 是受策略约束的编排进程。
tool 是能力端点。
session 是普通文件历史。
policy 是最小化、类 SELinux 的 allowlist。
CortexFS 协议适配层消除 provider 和 API 格式差异。
CortexFS 不把 provider/API 格式表达为根 ABI。
MCP 服务器是工具来源；MCP 能力是普通工具。
CortexFS 管理代理可见性、执行与共享，不管理框架配置格式。
```

## 冻结根规则

```text
root 只包含稳定对象分类
root 不镜像 provider、数据库、workflow、memory 或编排内部
MCP 不能成为根命名空间
MCP 配置、skills、项目规则、提示包都是普通可见文件
```

禁止作为根命名空间（示例）：

```text
skill/  memory/  mcp/  workflow/  chan/  job/  hook/  audit/  control/
```

这些概念可以以内嵌文件、会话数据或工具形式存在，但不能成为新的根类。

## 核心不变量

```text
Context 表示当前工作集，不是完整历史。
原始历史是持久化的。
Prompt 上下文是可丢弃、可重建的。
压缩不能破坏原始消息。
独立任务应在子代理中运行。
子代理默认归父代理所有，除非策略显式脱钩。
父代理结束时，受管子代理也结束。
Prompt 文本和 skill 元数据不授予权限。
权限来自 policy、路径、mount、uid/gid、mode bits。
机制层只执行主语、路径、mount 与 Linux 约束；注入的策略解释器只能进一步收紧权限。
```

## 架构优雅度

CortexFS 保留编码 agent 工具箱通常没有的 Unix 文件系统 ABI 与 Linux 权限模型。
其**内部优雅度标准**仍对标 Pi 工具包
([badlogic/pi-mono](https://github.com/badlogic/pi-mono))：严格分层、最小工具循环、
以事件事实而非 UI 决策为中心、包可独立使用、通过扩展而非第二套框架成长。
刻意省略与刻意实现同等重要。

### 两种心智模型

| 心智模型 | 拥有 | 不得拥有 |
| --- | --- | --- |
| Agent 内核 | model turn、工具调用/结果、取消、run 事件、context 投影 | TUI 布局、channel SDK、FUSE 投影、provider 线格式方言 |
| 交互 / 主机表面 | `ctx` / `tsh` / `ctxchat` / `ctxterm`、channel 适配器、web host | 第二套 agent 循环或平行根 ABI |

同一内核必须可嵌入终端、print、JSON/RPC 风格 socket 客户端与 channel 桥接。
新前端只适配现有 interaction 契约，不分叉循环。

### 分层包映射

Pi 的栈是 `ai → agent-core → coding-agent (+ tui)`。CortexFS 把同一重心映射到
Rust crate 与进程：

```text
Application / UX     ctx, tsh, ctxchat, ctxterm, channel adapters, web hosts
        ▲
Agent core           agent runtime + object runner（循环、工具、策略门）
        ▲
Protocol / AI        cortexfs-protocol、provider registry、model 投影
        ▲
Foundation           abi 类型、support fs/jsonl/layout、module 契约、paths
```

| CortexFS 重心 | Pi 类比 | 单一职责 |
| --- | --- | --- |
| `cortexfs-protocol` | `pi-ai` | provider 中立 request/event IR；无 HTTP、密钥或循环 |
| `cortexfs-module` + runner 循环 | `pi-agent-core` | 生命周期、能力、turn/tool 机制 |
| `cortexfs-runtime-client` | agent 事件/API 面 | 所有前端共用 `cortexfs.interaction/v1` |
| `ctx` / terminals / channels | `pi-coding-agent` / `pi-mom` | 会话、UX 模式、平台适配器 |
| FUSE `/ctx` 投影 | *（CortexFS 特有）* | 可检视对象类；不是 AI 数据库镜像 |

下层永不导入上层。协议代码不得认识 agent。Agent 内核不得认识 TUI 组件或
Discord 载荷。Channel 适配器翻译平台帧，并止于 interaction/channel socket 边界。

### 最小循环，持久事实

可执行内核保持同一小反馈循环：

```text
从持久会话事实构建可丢弃 context
  → 流式 model turn
  → 收集 tool calls
  → 授权并执行工具
  → 追加 observations
  → 直到最终答案或取消
```

其余全部叠在循环之外：

```text
skills / rules / templates     → context 输入，永不授予权限
extensions / modules / MCP     → 工具或适配器，永不成为根类
approvals / sandbox / policy   → 同一工具路径上的门控
compaction / summaries         → 重建 prompt；永不改写原始历史
frontends                      → 订阅事件；永不拥有循环
```

会话文件回答“发生了什么？”Prompt context 回答“模型下一步需要什么？”
二者保持分离，对应 Pi 的 session 树与 `convertToLlm` 投影。

### 事件即事实

每一层发出可关联的类型化事实（`run`、`request_id`、tool id、status）。
终端渲染它们，JSON 客户端序列化它们，会话记录器追加它们，测试断言顺序。
呈现层不得回写权限或历史 schema。Interaction 与 channel socket 已遵循此规则；
新表面必须复用这些事件族，而不是发明平行控制平面。

### 可组合与刻意省略

包必须可独立有用：

```text
仅 cortexfs-protocol     → 转码 provider 格式
仅 runtime-client        → 说 interaction 帧
仅 tool-sdk / agent-sdk  → 实现一个能力进程
仅 channel-sdk           → 隔离一个平台传输
```

刻意省略（反框架）：

```text
无 workflow / hook / job / memory 根
无烘焙进循环的 plan-mode 产品面
无进入 /ctx 路径或 agent 分支的 provider 方言
无把每个 channel SDK 装进同一进程的巨型 harness
无后台 watcher 或热重载控制平面
```

特化属于对象、模块、skills 与适配器。主机只保留稳定原语：文件、socket、
策略、原子 rename 与进程重启。

### 扩展点（反框架）

Pi 在 AI、agent、application 三层扩展，而不增长插件根。CortexFS 用 Unix 边界
表达同一想法：

| 层 | 用什么扩展 | 不得变成 |
| --- | --- | --- |
| Protocol / AI | provider 适配器、`cortexfs-protocol` 路由、model `driver` / `cap` 投影 | `/ctx` 上的 provider 方言路径，或 agent 按供应商名分支 |
| Agent core | `cortexfs-module` 生命周期、Tool/Agent SDK、策略解释器、context 变换 | 循环内 plan 板、hook DAG，或第二套编排 ABI |
| Application | 单文件包（[extensions.md](extensions.md)）、skills/rules 文件、channel 适配器、terminal/web/IM 客户端 | `/ctx/skill`、`/ctx/mcp`、`/ctx/workflow`，或常驻插件守护进程 |

树内已有的具体表面：

```text
cortexfs.module.socket/v1     进程隔离的 module 生命周期
Tool SDK / Agent SDK          一个可执行能力或 agent 步骤
cortexfs.package/v1           编写输入 → 普通 agent/tool 对象
cortexfs.interaction/v1       所有前端说同一套 request/event 事实
cortexfs.channel.socket/v1    平台适配器留在 agent 循环之外
skills / AGENTS.md / rules    可丢弃 context 输入，永不授予权限
MCP via ctxmcp                普通工具；永不成为根类
```

结构止于稳定原语与生命周期边界。新行为是新对象、module、skill 或适配器——
不是新的根目录，也不是热加载的进程内扩展宿主。见
[module-abi.md](spec/module-abi.md) 与 [extensions.md](extensions.md)。

## 身份、生命周期与传输

CortexFS 使用四种身份，不应折叠到“agent daemon”或第二条生命周期树中：

| 层 | 稳定身份 | Owner |
| --- | --- | --- |
| 定义 | `agent/<name>` + `agent/<name>.d/` | reference tree |
| 运行时实例 | supervisor unit + invocation receipt | runtime/supervisor |
| 会话 | `home/<uid>/agent/<name>/session/<session>/` | durable files |
| 运行段 | 会话事件中的 128 位随机 run id | session recorder |

定义说明 Agent 如何运行；运行时实例说明当前是哪一组进程在实现该定义。
会话拥有可持久化的人类与 Agent 历史。run 是该会话内一次受界限的执行。

`agent/<name>.d/meta.json` 可以保留最近一次与检视和安全清理相关的
receipt 约束事实。`status`、`pid`、`log` 是汇总投影，不改变 Agent 的定义身份，
也不构成独立进程监督者。

不要仅为了镜像 systemd 与 receipt 已拥有的进程状态就新增 `instances/`。
若将来要做多实例特性，必须先定义不能由现有
`agent/session/unit/run` 四元组表达的身份，再指定唯一权威生命周期 owner 与迁移路径。

Socket 是传输方式。长连接 socket 位于 `/run`，`agent/<name>.sock` 与
`session/<session>/terminal/main.sock` 是稳定 ABI 条目或别名，用于发现这些传输。
socket 的存在与否不决定对象身份、会话持久性或进程归属。

总结规则：

```text
object 定义身份
supervisor receipt 定义进程生命周期
普通文件定义持久状态
socket 提供可选传输
```

## 模型与 context 边界

模型对象是稳定的 provider/model 身份。其 `driver` 控制用于按场景选择可替换适配器，
`cap` 与 `limit` 只投影 provider 中立事实。Agent 与 context 逻辑消费这些投影，
不得基于 provider 名称、API 格式或模型品牌分支处理。

能力数据采取保守策略。硬约束按 Model ABI 的优先级顺序为：
主机显式 per-model 配置 → 已校验目录 → `unknown`。稳定 `cap` 词条是适配器投影；
不支持或不可信事实应被省略。未来若要覆盖每模型能力或主机侧探测，必须通过版本化
Model ABI 变更；不得变成模型调用副作用、后台 watcher 或第二套配置源。有效证据应落入同一
验证后的 `cap`/`limit` 投影，否则仅保留为诊断。

Context 组装使用模型硬 `limit` 与 Agent 的衰减 `window` 控制。原始会话历史保持不变，
渲染提示可以使用最近尾部、摘要、规则、skills 与已加载工具元数据。模型切换因此重建提示
上下文，不会重写历史，也不会给每个 Agent“记忆”模型专用案例表。

## 事物位置

打包主机在以下位置维护版本化持久树：
`/var/lib/cortexfs/storage/generations/<generation>`，并通过原子
`/var/lib/cortexfs/storage/current` 符号链接暴露选定树。
systemd 重启时，`ctx storage update` 克隆当前 generation，应用并校验下一版
`bin/cortexfs.bootstrap.json` 的 `tree_version`，再切换 `current`。失败阶段会让
`current` 保持不变。这是重启边界，不是 watcher、poller 或热重载；`/ctx` ABI 形状不变。
打包本地生成根文件，generation 不是可分发工件。systemd 重启路径会在停止消费者后，
显式使用 `--prune` 删除非当前 generation。不存在后台 generation GC。
mount 与 agent runtime 在进程启动时仅解析一次 `current`，并在整个生命周期里固定使用，
包括 mount 缓存刷新。短生命周期对象运行器调用可每次解析当下 generation。

| 地点 | 路径形状 | 角色 |
| --- | --- | --- |
| 控制 | `/ctx/agent/<name>.d/*` | policy、mount、cwd、system.md |
| 代理主目录 | `/ctx/home/<uid>/agent/<name>/` | 会话、数据、缓存、日志 |
| 会话 | `.../session/<session>/` | 消息、事件、context、加载快照 |
| 运行时 IPC | `/run/user/<uid>/cortexfs/...` | 仅终端 socket |

沙箱映射（典型）：

```text
/ctx/home/<uid>/agent/<name>  →  HOME=/home/agent   (rw)
caller project cwd               →  /workspace         (rw，默认 cwd)
/ctx                            →  /ctx               (通常 ro)
```

`/run` 用于 socket。Agent 的 cwd 通常是 `/workspace`。会话文件位于 agent home，而非
`/run` 下。

## 提示构建可观测性

对象运行器构建一次运行提示时会尽量写入：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/AGENTS.md
/ctx/home/<uid>/agent/<agent>/session/<session>/SKILLS.md
```

```text
AGENTS.md   合并后的规则快照（内容与 {{rules}} 一致）
SKILLS.md   仅技能元数据（name、description、path）
```

这些是普通会话文件，不是权限文件。完整 skill 内容仍保留在列出的 `SKILL.md`
路径。实现位置：`agent/prompt/snapshot.rs`。

## 工程品味

由上文优雅度标准导出：

```text
优先短名而非长短语
每个模块与每个 crate 只做单一明确职责
先复用再新增 helper
不重复定义 Empty/Missing/Invalid 的平行枚举
不新增编排类第二根 ABI
不新增后台 watcher、poller 或 hot-reload 子命令
Git 提交或进程重启是开发刷新边界
控制面写入使用临时文件原子重命名
历史与快照使用普通文件
下层永不导入上层
事件是事实；UI 只订阅
在稳定边界确实需要之前，把复杂度留在循环之外
```

模块命名见 [naming-guide.md](naming-guide.md)。优先单 token stem 的文件名
（如 `snapshot.rs`）；模块文件 stem 不允许新增 `-` 或 `_`。

## 内部代码架构

产品规则定义了 `/ctx` 的“是什么”。Rust 树如何分层
（进程角色、crate/feature 划分、模块依赖方向、错误层次、迁移阶段）
见 [internal-architecture.md](internal-architecture.md)。

在进行大规模重构（crate 拆分、executor 错误迁移、FUSE 与 object 边界变更）前，
先阅读该文档。不要因为“结构更好看”而新增根 ABI 类、workflow 引擎或
后台观察者入口。

## 按顺序阅读规格

```text
architecture.md              # 优雅度标准、扩展点
internal-architecture.md     # crate/模块分层
spec/README.md
spec/root-abi.md
spec/fuse.md
spec/object-abi.md
spec/model-abi.md
spec/session-abi.md
spec/agent-tool-security.md
spec/agent-runtime.md
spec/module-abi.md
spec/tool-policy-abi.md
spec/ctx-coreutils.md
spec/rolling-upgrades.md
extensions.md
```

## 稳定 ABI 红线

```text
不要让 /ctx 变成 AI 平台数据库的目录镜像。
它应保持小、硬、单调且可脚本化。
内部优雅度对标 Pi，但不把 Pi 的产品表面导入为根 ABI。
```
