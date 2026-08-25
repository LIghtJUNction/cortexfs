---
id: developing-cortexfs
title: 扩展 CortexFS
sidebar_label: 扩展 CortexFS
---

# 扩展 CortexFS

先给一个规则：CortexFS 的扩展点是当前规范里的对象、套接字、控制文件和工具提交语义。它们不是新的根目录或新的工作流入口。该反框架落点与 [architecture.md](architecture.md) 中的 Pi 优雅度标准一致。

常规场景下，请先走 [One-file Extensions](extensions.md) 的路径：将工具与可执行代理放在一个包目录里，用一个 `cortexfs.toml` 描述它们，先运行 `ctx install --check ./package`，再用 `ctx install ./package` 安装。该包只是编写便利层，安装依然使用同一套“按哈希绑定、原子发布”的对象机制，以及同一套 `agent/<name>.d/*` / `tool/<name>.d/*` ABI。

## 先读边界

建议阅读顺序：

```text
DESIGN.md
architecture.md          # 对标 Pi 的优雅度标准与扩展点
internal-architecture.md # crate/模块分层规则
spec/README.md
spec/root-abi.md
spec/object-abi.md
spec/model-abi.md
spec/module-abi.md
spec/session-abi.md
spec/tool-policy-abi.md
spec/ctx-coreutils.md
extensions.md
aimock-testing.md
```

根 ABI 仅包含：

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
```

不要新增顶层目录，如 `provider`、`workflow`、`job`、`hook`、`mcp`、`skill` 或 `audit`。

## 开发思维模型

CortexFS 的扩展工作从文件操作开始，而不是框架集成，但这些文件不总是磁盘文件。

`/ctx` 下的路径可能是磁盘文件，也可能是内存投影，由当前代理、会话、权限与上下文共同决定。传统代理架构常常暴露额外调试 API、转储 JSON，或反复将运行时状态写入文件，方便开发者查看上下文。磁盘文件会带来 I/O 和同步成本；tmpfs 快但短暂。FUSE 让这些状态以文件形态出现：当路径未被打开、统计或读取时无需物化；需要检查时使用普通 Unix 工具即可。

这正是 CortexFS 的核心形态：隐藏的运行时状态变成“所见即所得”的文件视图，同时仍能深度定制。代理无需新的框架，只需要高阶对象：文件、套接字、可执行工具和会话。

```text
write agent/<name>.d/*     配置身份、模型、权限、挂载、工具路径
connect agent/<name>.sock  发送 JSONL 会话请求
execute tool/<name>        运行受策略约束的能力
read session/*             查看历史、事件、最新输出、上下文包
read context/*             查看工作集、文件引用、子任务结果
read xattr/stat            查看文件类型、来源、token 预估、安全事实
```

一个最小化代理运行时可以是单个可执行文件：从 stdin 或
套接字读取请求，选择 `agent/<name>.d/model`，并输出稳定的事件帧。更完整的运行时可以加入工具循环、上下文打包、子代理编排、provider 适配，但仍落在同样的对象、套接字和语义上。

对图片、PDF、音频、归档及其他非文本输入，不要把字节塞进 prompt，也不要发明单独上传 API。将文件放到代理可见位置，再在对话中通过路径引用：

```bash
ctx agent start executor --session default --mount "$PWD" /workspace rw
ctx send executor "Analyze /workspace/assets/screenshot.png and compare it with /workspace/docs/DESIGN.md"
```

若材料在多个代理或会话间共享，请使用共享空间：

```bash
mkdir -p "$(ctx path shared project-a)/input"
cp screenshot.png "$(ctx path shared project-a)/input/"
ctx agent new reviewer --shared project-a:read
ctx send reviewer "Inspect /ctx/shared/project-a/input/screenshot.png"
```

运行时只需把这些路径记录到 `context/refs.jsonl` 或上下文包。读取图片字节、估算 token、渲染缩略图、调用视觉模型应通过相应工具或 provider 适配器按需进行。

## 扩展工具

工具是可执行能力端点。用户可见：

```text
/ctx/tool/<name>
/ctx/tool/<name>.d/
```

执行可以在 Rust 执行器、外部程序或运行时内部进行，但权限仍由代理视图、`CTX_PATH` 与 policy 决定。

对于异步工具或可检索结果的工具，使用统一的提交语义：

```text
1. 写入临时文件。
2. 在同一目录中原子重命名为 *.req.json。
3. 从 outbox 读取结果。
4. 将事实追加到 audit。
```

这使工具开发保持 Unix 风格。CLI 模式使用 argv/stdin/stdout；代理原生模式可使用工具 SDK 的结构化 JSON 与进程内调用。两种模式共享同一套 `.d/schema`、`.d/policy` 与可见性规则。

## 扩展代理

代理是受策略约束的编排者。稳定路径是：

```text
/ctx/agent/<name>
/ctx/agent/<name>.sock
/ctx/agent/<name>.d/
/ctx/home/<uid>/agent/<name>/session/
```

在某些部署中，`/ctx/agent/<name>.sock` 是到用户运行时路径（如 `/run/user/<uid>/cortexfs/agent/...`）的所有者授权符号链接；在其他部署中，它可能是直接套接字节点。请先探测当前挂载，再假定单一实现形式。

代理可以组织工具循环、上下文、子任务与交接，但这些编排概念不应形成新的根 ABI。

### 代理树

根代理是可继承的根身份。子代理不是复制一个进程，而是缩小可见世界：

```text
architect
├── executor
└── product-manager
```

父代理可创建子代理，但子代理的模型、工具、挂载、共享空间、uid/gid/组和策略都必须是父权限的子集。子任务交接、结果、引用与生命周期记录位于父会话下的 `context/child/<id>/`。所属子任务随父任务失效一起取消；脱离的子任务需要显式策略。

### 终端：ctxterm 与 tsh

当前 `ctx agent start` 的终端链路是：

```text
systemd-run --user
bwrap sandbox
ctxterm
tsh
```

默认会在沙箱内将调用者当前目录挂载到 `/workspace`。额外挂载必须使用 `--mount SOURCE TARGET ro|rw` 显式声明；`TARGET` 不得替换 `/` 或 `/ctx`。该路径是代理终端实现，不是新的后台监视器、轮询循环或热重载子命令。

`ctxterm` 持有 PTY，并通过会话终端套接字暴露 `watch` 与 `attach`：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

`tsh` 只通过 `CTX_PATH` 查找工具；不会回退到主机 `PATH`。独立人类会话会在继承前读取 `CTX_HOME/.tshrc`，并由该文件的 `CTX_PATH=...` 给出数据化配置。在代理终端内，运行时提供的 `CTX_PATH` 仍是权威。

这种分离是故意的：

```text
ctxterm  持有 PTY 生命周期，并提供 watch/attach 与多观察者终端访问
tsh      发现工具、加载/固定它们，并通过 CTX_PATH 调用能力
bash     只是普通工具，在可见且允许时可使用
tmux     也是普通工具，适用于长时间面板或后台任务
```

对代理默认可见的原生工具是 `tsh`。仅因提示词提到某工具并不会使其出现；工具必须通过 `tsh tools`、`tsh load TOOL`、`tsh pin TOOL` 与 `tsh TOOL ARG...` 进入工作集。

### 上下文窗口管理

CortexFS 将上下文视为工作集，而非真相来源：

```text
messages.jsonl     持久会话事实
events.jsonl       持久运行时事实
latest.md          最近输出视图（可重建）
context/pack.md    当前工作集（可重建）
context/refs.jsonl 已选择文件、子任务结果、搜索结果
```

代理或其他用户态运行时选择内容、构建 pack，并通过同目录原子替换写入 `context/pack.json` 与 `context/pack.md`。CortexFS 持有 pack 形状和来源校验、`/ctx` 可见性和文件耐久性；它不会代替运行时选择 prompt、估算预算，也不会替运行时重建 pack。

这是 0.2.0 时代的破坏性 API 退役：公共
`rebuild_context_pack`、`ContextPackBuildError`、`ContextPackBuild` 与
`ContextPackBuiltItem` 符号（含相关方法）已移除。用户空间实现者仍可用
`inspect_context_pack_json` 与 `validate_context_pack_source` 验证其输出。

提示构建会融合代理指令、AGENTS.md 规则、技能元数据、工具注入、消息历史与运行时契约。技能元数据起始只包含 `name`、`description` 与 `SKILL.md path`，最多使用上下文窗口的 2%，若窗口大小未知则硬上限 8000 字符。超预算先缩短描述，仍超预算则省略部分技能并给出警告。完整 `SKILL.md` 内容仅在技能被选中后读取。

### 权威控制

提示与 schema 不是权限系统。有效权限始终是多层交集：

```text
mount/chroot 可见性
Linux uid/gid/group 与 mode bits
CortexFS label + policy v0
CTX_PATH 工具可见性
工具可执行文件元数据
noexec 挂载策略
```

例如，读取一个文件不意味着就能执行其相关工具；看到某工具文件并不意味着 policy 允许调用它；提示里写“you may use shell”也不能绕过 `tsh` 或策略。

## 扩展 Provider 或本地模型

provider/model 设计必须保持中立。CortexFS 不会把任何供应商写成核心默认路径，也不会把 Ollama 作为核心特殊分支。

本地轻量 live-test 固定 fixture 为：

```text
smollm2:135m
```

如果该模型不存在，请提示用户安装/拉取，不要静默切换模型。仅在用户明确要求测试其配置供应商/聚合 API 时，才使用现有 provider registry、route、secret 状态和统一提交语义。

Provider API key 解析顺序为：

```text
1. 提供者环境变量候选项（如果设置）
2. root 拥有的 CortexFS 系统密钥存储
3. 未配置，返回稳定错误
```

不要将密钥写入 `/ctx/model/*`、`.d/default` 或任何 ABI 文件。OAuth access token 遵循同一原则：provider 适配器从系统密钥存储读取 secret state。provider 配置可声明 Authorization Code + PKCE 元数据。默认 access token 存储在
`service=cortexfs:<provider> account=oauth:access`，refresh token 存储在
`account=oauth:refresh`。PKCE verifier、state、access token、refresh token
都不得写入 `/ctx/model/*`、`.d/default` 或任何其他 ABI 文件。

若要在不调用外部云 API 的情况下测试 OpenAI 兼容路径，请使用本仓库的 aimock fixture：

```bash
npm install
npm run aimock
npm run aimock:smoke
```

详见 [AIMock Testing](aimock-testing.md)。这是本地测试 fixture，不是新的 `/ctx/provider` 根命名空间。

多 API 兼容边界是：

```text
/ctx/model/main                    稳定默认模型别名
/ctx/model/<provider>/<model>      provider 适配器投射的模型对象
model/<name>.d/driver              driver/route 元数据
provider registry/cache/secret store   运行时状态，不是根 ABI
```

切换 provider 时，用户更新模型别名或路由即可，代理可继续使用 `use model:main`。provider 兼容性不应渗透到 agent、tool、session 或权限模型。

## 性能设计

CortexFS 高效的原因在于边界小：

```text
对象发现         目录读取与短控制文件
模型/工具执行    文件执行或 Unix 套接字
会话             JSONL 帧流
上下文打包       持久历史 + 可重建工作集
工具上下文       显式 load/pin；未 pin 条目由 W-TinyLFU 回收
权限校验         静态 mount/policy/mode-bit 交集
```

根 ABI 只有少量对象类，因此 provider、数据库、workflow、MCP 服务和临时 job 不会各自变成新的根目录。代理运行时可保持会话内可见工具的快速内存投影，耐久状态仍是普通文件和稳定事件。

## 本地校验

常用校验：

```bash
cargo test
npm --prefix docs-site run build
```

固定的 FUSE 集成测试挂载点是：

```text
tests/mounts/cortexfs
```

该目录只作为本地测试挂载点，不得放置源码、fixture 或持久化数据。

## 参考项目与相近实现（关键词检索）

- [tursodatabase/agentfs](https://github.com/tursodatabase/agentfs)
- [modelcontextprotocol filesystem server](https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem)
- [rust-mcp-stack/rust-mcp-filesystem](https://github.com/rust-mcp-stack/rust-mcp-filesystem)
- [opencrust multi-agent runtime](https://github.com/opencrust-org/opencrust)

### 相关 issue / PR

- CortexFS
  - [#89](https://github.com/LIghtJUNction/cortexfs/pull/89)
  - [#88](https://github.com/LIghtJUNction/cortexfs/pull/88)
  - [#87](https://github.com/LIghtJUNction/cortexfs/pull/87)
- modelcontextprotocol/filesystem server
  - [#3232](https://github.com/modelcontextprotocol/servers/issues/3232)
  - [#3402](https://github.com/modelcontextprotocol/servers/issues/3402)
  - [#4208](https://github.com/modelcontextprotocol/servers/issues/4208)

### 相似代码检索关键词

- `provider registry` + `object` + `policy`
- `Fuse` + `socket runtime` + `jsonl`
- `atomic rename .req.json` + `outbox` + `audit append`
- `model alias` + `route` + `secret store`
