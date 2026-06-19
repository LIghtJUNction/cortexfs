# CortexFS

<div align="center">
  <p>
    <a href="Cargo.toml"><img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-f46623?logo=rust&logoColor=white" /></a>
    <a href="Cargo.toml"><img alt="MIT license" src="https://img.shields.io/badge/License-MIT-green.svg" /></a>
    <a href=".github/workflows/pages.yml"><img alt="Docs site" src="https://img.shields.io/badge/Docs-Pages-7b68ee" /></a>
    <a href="AGENTS.md"><img alt="Agent docs" src="https://img.shields.io/badge/Agent%20docs-available-0ea5e9" /></a>
  </p>
</div>

<p align="center">
  <a href="#overview">Overview</a>
  · <a href="#quick-start">Quick Start</a>
  · <a href="#文件系统形状">中文说明</a>
</p>

CortexFS 是一个专为 Linux 设计的 Rust FUSE 文件系统。它把 AI API
format、provider、模型、请求、响应、对话线程、工具、MCP、skill、agent、记忆、集群状态和审计记录暴露为普通文件。

## Overview

CortexFS turns AI infrastructure into a filesystem ABI. Instead of hiding state behind an app UI, it exposes formats, providers, models, threads, tools, memory, audit logs, and control surfaces as files that shell tools can inspect and mutate.

Typical use cases:

- inspect model/provider state with `cat`
- submit requests with atomic `mv`
- audit behavior with JSONL logs
- automate workflows with standard Unix tooling

## Quick Start

```bash
paru -S cortexfs-git
cortex status
```

For developers:

```bash
cargo build --locked --workspace
cargo run -p cortex-cli -- status
```

它的目标不是再做一个黑箱 agent 框架，而是把 agent 基础设施做成可检查、可脚本化、可审计的 Linux 文件系统接口。挂载树就是公开 ABI：用 `cat` 读取状态，用 `mv` 原子提交请求，用 `printf` 写控制节点，用 `find`、`xargs`、`jq`、`awk` 等普通 Unix 工具批处理。

很多挂载树中的文件并不会落盘。它们是由 `cortexfs`/`cortexd` 运行时投影出来的 in-memory 虚拟文件，只在挂载实例中存在。这一点参考 `/proc` 和 `sysfs`：文件系统是内核/VFS 风格的状态视图和控制面，不是普通数据目录。

CortexFS 的核心提交语义只有一条：先写临时文件，再用同目录原子 `rename` 提交为 `*.req.json`，结果从 outbox 读取，事实进入 audit。它不再把 channel、job、hook 作为新的文件系统抽象；provider、route、inbox、outbox 和 audit 已经足够表达这些能力。

> 当前版本：`0.1.0`

## 定位

CortexFS 是“AI API 格式的 FUSE 文件系统”，不是某一个 provider 的客户端。

它把不同生态拆成两层：

- `format/`：协议形状，例如 `openai.chat`、`openai.responses`、`anthropic.messages`、`google.generate_content`。
- `provider/`：后端实例，例如官方账号、中转站、使用 OpenAI 格式的厂商、本地推理运行时或未来的 adapter。

现实中 Kimi、MiniMax、中转站、本地模型服务可能都支持 `openai.chat` 或 `openai.responses`；CortexFS 不把厂商品牌硬编码成核心路径，而是让 provider 声明自己支持哪些 format、模型、账号类型、健康状态和路由策略。

## 项目边界

- `cortexfs` 只做 FUSE/VFS 投影；远程 provider 调用、MCP 通信、tool loop、记忆检索、审计导出和调度执行属于 `cortexd`/execution plane。
- 文件系统树是公开 ABI，不是产品 UI。路径、文件名、读写语义、权限语义和错误语义必须稳定、可文档化、可测试。
- 提交入口只保留通用文件队列：写临时文件，同目录原子 rename 成 `*.req.json`，从 outbox 读结果，向 audit 追加事实。
- 不把 channel、job、hook、workflow 做成核心目录；外部编排器应把 run、step、trigger 等状态放进请求 JSON、thread metadata、audit context 或自己的状态库。
- provider/model 必须保持中立。本地 live test 当前使用 Ollama `smollm2:135m`，但它只是测试 fixture，不是核心默认 provider、核心能力或特殊分支。
- 开发期刷新以 Git commit 为唯一事件边界；已挂载实例不会热更新，新 ABI 需要提交后重建并重新挂载。

## 首版能力

`0.1.0` 建立了第一版可验证的 FUSE ABI、Rust workspace、严格 lint 规则和 provider-neutral 的运行时投影。

已经具备的核心面：

- FUSE 挂载树：`format`、`provider`、`model`、`home`、`group`、`shared`、`ext`、`space`、`agent`、`cluster`、`mcp`、`skill`、`tool`、`memory`、`vector`、`db`、`audit`、`control`。
- API format：`openai.chat`、`openai.responses`、`anthropic.messages`、`google.generate_content`。
- provider/model 发现：通过 `count`、`list`、`format`、`url/*`、`enabled/*`、`health/*`、`model/*` 等小文本文件读取。
- 多 provider 配置：向 `provider/inbox/*.req.json` 原子提交 provider instance 配置，向 `provider/<id>/secrets/inbox/*.req.json` 导入 API key；明文 key 保存进系统 Secret Service，挂载树只暴露 secret reference。
- 文件式 API 调用：写临时文件，原子 rename 到 `inbox/*.req.json` 入队，再由 `control/drain` 或 daemon worker 执行。
- route-aware audit：请求、拒绝、执行、错误都会写入 `audit/events.jsonl`，包含 provider、model、decision、fingerprint 等字段。
- thread 视图：`messages.jsonl`、`latest.md`、`fingerprint`、`state`、`tool-loop/steps.jsonl` 和预留的 `io.sock` fast path；MCP tool 调用会进入同一条 permission/tool call/tool result 轨迹。
- cluster 任务骨架：`queue/default/{pending,running,done,failed}`、`task/<id>/state`、`events.jsonl`、失败任务 `retry` 控制节点。
- 多用户空间：`home/<uid>/...` 是权限、模型可见性、路由、记忆、审计和导出的边界。
- 外部主体模型：为聊天平台、机器人平台、Webhook 等非 Linux 用户预留 `external subject` 身份上下文。
- 记忆与导出：分层记忆、`memory/index/default/refresh`、JSONL 导出、训练数据友好的对话和 tool trace 投影。
- 本地 live test：使用本机轻量模型 fixture 验证 provider 调用，不依赖云 API。
- `cortex daemon --once`：用一次性请求验证 daemon execution plane 的入队、执行和原生 JSON 响应路径。

## 当前非目标

这些能力在设计中已经留出 ABI，但不是首版完成状态：

- 长驻 `cortexd` daemon、后台队列 worker 和生命周期管理。
- 生产级 HTTP 本地 API。
- OAuth/session 管理和完整密钥轮转策略。
- 完整 MCP server 生命周期管理。
- 真正的集群调度、跨机器 worker、成本控制和重试系统。
- 完整向量数据库/PG 持久化实现。
- 内置 workflow/job/hook/channel DSL；外部编排器、systemd timer、cron、CI 或 webhook 应直接写入通用 inbox。
- 非 Linux 平台支持。

## 文件系统形状

挂载根目录当前形状：

```text
/
  status
  cap/
  format/
  provider/
  model/
  home/
  group/
  shared/
  ext/
  space/
  agent/
  cluster/
  mcp/
  skill/
  tool/
  memory/
  vector/
  db/
  audit/
  control/
```

开发期只保留当前 ABI。脚本和集成只能依赖上面这组单数、短名词目录；同一对象不能再暴露第二套入口或复数形式。挂载树不是可扩展数据目录；对未声明 ABI 目录执行 `mkdir` 会返回 EROFS。尤其不要依赖 `/ctx/chan`、`home/<uid>/job` 或 `home/<uid>/hook` 这类已移除的上层抽象。

小文件约定：

- 一个文件一个简单值。
- 多值列表一行一个。
- 布尔值使用 `0`/`1`。
- JSON 用于原生 API request/response。
- JSONL 用于消息、审计、tool loop、训练数据导出。
- `.sock` 只作为实时 fast path，文件树仍然是可审计 source of truth。

## 安装与快速试用

Arch Linux 用户可以直接从 AUR 安装 `cortexfs-git`：

```bash
paru -S cortexfs-git
cortex status
```

从源码构建适合开发者和 CI：

```bash
cargo build --locked --workspace
cargo run -p cortex-cli -- status
```

## 实机安装目录

推荐实机挂载点：

```text
/ctx
```

`ctx` 可以理解为 `cortex` 的缩写，也可以理解为 `context` 的缩写。这个路径短、稳定、适合 shell 脚本、agent 进程和其他本地软件长期引用。

每个 Linux 用户都有自己的 home 入口：

```bash
export CTX_HOME="/ctx/home/$(id -u)"
```

`$CTX_HOME` 类似 `/home/$USER`，但指向 CortexFS 内部的用户空间。比如 `$CTX_HOME/api`、`$CTX_HOME/thread`、`$CTX_HOME/memory`、`$CTX_HOME/export` 分别对应当前用户可用的 API、对话、记忆和导出视图。

`home/<uid>` 是唯一用户工作入口。通过 `$CTX_HOME` 提交请求、写 thread、读 memory，都会落到同一套队列、路由、审计和导出流。

目录命名遵循 Linux 风格：普通名词、短路径、单一入口。用户路径使用 `/ctx/home/<uid>`；共享空间使用 `/ctx/shared/<name>`；外部平台身份使用 `/ctx/ext/<platform>/...`。

示例：

```bash
cortex start
export CTX_HOME="/ctx/home/$(id -u)"
```

后台挂载使用 AUR 包安装的 systemd 模板服务，直接用 CLI 启动。`cortex start` 会请求系统授权管理一个 `cortexfs@<owner>.service` 实例，服务自动加载 FUSE、清理坏挂载、创建 `/ctx` 并以 multi-user FUSE 模式共享给本机用户：

```bash
cortex start
systemctl status "cortexfs@$USER.service"
```

同一台机器上不要为多个 Linux 用户同时启动多个 `/ctx` 挂载实例；`/ctx` 是系统级单挂载点。需要手动前台调试时，使用同一条 multi-user mount 参数：

```bash
cortex mount --multi-user /ctx
```

`space/` 是只读安全上下文索引；日常脚本使用 `/ctx/home/<uid>`、`/ctx/shared`、`/ctx/ext` 这些直接入口。开发期不提供 `spaces/` 目录，也不提供 `space/users/<uid>` 这类第二入口。

## 本地测试挂载

```bash
mkdir -p tests/mounts/cortexfs
cargo run -p cortex-cli -- mount tests/mounts/cortexfs
```

`tests/mounts/cortexfs` 只用于仓库内集成测试，不是实机推荐安装目录。

已挂载实例不会热更新。开发期以 Git commit 作为唯一刷新边界；要观察新 ABI，需要在提交后重建并重新挂载。

查看挂载树：

```bash
mnt=tests/mounts/cortexfs

cat "$mnt/status"
cat "$mnt/cap/format"
cat "$mnt/cap/provider"
cat "$mnt/cap/tool"
find "$mnt" -maxdepth 2 -type f | sort
```

查询 provider 和模型：

```bash
mnt=tests/mounts/cortexfs

cat "$mnt/provider/count"
cat "$mnt/provider/list"
cat "$mnt/model/list"
cat "$mnt/format/openai.chat/model/list"
```

查询某个 Linux 用户实际可用的模型：

```bash
mnt=tests/mounts/cortexfs
CTX_HOME="$mnt/home/$(id -u)"

cat "$CTX_HOME/model/count"
cat "$CTX_HOME/model/list"
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
```

## 文件式 API 调用

CortexFS 的文件式提交规则是：普通 `write()` 只写入内容，不触发 provider；只有同目录原子 rename 到 `inbox/*.req.json` 才表示提交。FUSE 回调只完成入队和派生元数据，不在 `rename()` 里调用远程 provider；执行由 `control/drain` 或后续 daemon worker 完成。

```bash
mnt=tests/mounts/cortexfs
CTX_HOME="$mnt/home/$(id -u)"
api="$CTX_HOME/api/openai.chat"

printf '%s\n' '{"messages":[{"role":"user","content":"Reply with cortexfs-ok"}]}' \
  > "$api/inbox/001.tmp"

mv "$api/inbox/001.tmp" "$api/inbox/001.req.json"
printf '1\n' > "$mnt/control/drain"

cat "$api/outbox/001.resp.json"
```

失败时读取：

```bash
cat "$api/outbox/001.error"
```

request id 来自文件名 stem，例如 `001.req.json` 的 request id 是 `001`。这个设计让 shell 批处理天然可用：

```bash
for f in requests/*.json; do
  id="$(basename "$f" .json)"
  cp "$f" "$api/inbox/$id.tmp"
  mv "$api/inbox/$id.tmp" "$api/inbox/$id.req.json"
done

printf '1\n' > "$mnt/control/drain"
find "$api/outbox" -name '*.resp.json' -print -exec jq . {} \;
```

外部触发器也走同一条路：systemd timer、cron、CI、webhook bridge 或 workflow engine 只需要把请求 JSON 写成临时文件，再 `mv` 到对应 inbox。CortexFS 不为它们新增 `job/`、`hook/`、`workflow/` 目录。

## Agent 可观测性

传统 agent 很容易变成黑箱。CortexFS 要求 agent 看到了什么、能调用什么、用了什么工具、写了什么记忆，都能通过文件系统检查。

```bash
mnt=tests/mounts/cortexfs
CTX_HOME="$mnt/home/$(id -u)"
thread="$CTX_HOME/thread/demo"

cat "$thread/messages.jsonl"
cat "$thread/latest.md"
cat "$thread/fingerprint"
cat "$thread/tool-loop/steps.jsonl"

cat "$mnt/tool/list"
cat "$mnt/mcp/tool/list"
cat "$mnt/skill/installed/cortexfs-test/SKILL.md"
cat "$mnt/agent/helper/memory/scope"
cat "$mnt/agent/helper/memory/search"
```

后续实时通信会使用 socket fast path，例如 `thread/<id>/io.sock`。socket 必须和文件式提交进入同一条内部管线：同一套 policy、route、secret resolve、store、audit 和 export，不能形成不可审计旁路。

本地统一 API 的元数据在当前用户工作入口下暴露：

```bash
cat "$CTX_HOME/api/endpoints"
cat "$CTX_HOME/api/transport"
cat "$CTX_HOME/api/pipeline"
cat "$CTX_HOME/api/store"
cat "$CTX_HOME/api/policy"
cat "$CTX_HOME/api/audit"
cat "$CTX_HOME/api/unix/path"
```

`api/http` 和 `api/unix/api.sock` 只是同一执行面的不同传输入口，必须进入同一条 `api/pipeline`；它们不能拥有独立的 provider 调用路径。

## 外部软件集成

CortexFS 可以被 workflow engine、agent runtime、数字人系统和批处理脚本当作本地 AI 执行面使用，但它不会为某个上层项目增加专属目录。集成方应该依赖发现文件和通用提交规则：

```bash
CTX_HOME="/ctx/home/$(id -u)"

cat "$CTX_HOME/model/list"
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"

mv req.tmp "$CTX_HOME/api/openai.chat/inbox/run-001.req.json"
cat "$CTX_HOME/api/openai.chat/outbox/run-001.fingerprint"
cat /ctx/audit/events.jsonl
```

同一个请求无论来自文件、HTTP 还是 Unix socket，都必须进入同一条管线，并产生同一个 request id、fingerprint、route metadata、policy decision、audit event 和 export row。socket 只是低延迟 fast path，不是绕过文件 ABI 的旁路。

不要写死 `home/1000`、`agent/helper`、`ext/chat/room/888888` 这类示例路径；正式模式是 `home/<uid>`、`agent/<agent-id>`、`ext/<platform>/...`，并通过 `count`、`list`、`status`、`route`、`model` 等小文件发现实际可用对象。

如果集成方需要表达“任务”“步骤”“触发器”，把这些字段放进请求 JSON、thread metadata 或外部系统自己的状态库；CortexFS 只负责通用的 provider/route/policy/queue/audit/export 执行面。

### Bun 客户端模板

仓库提供一个零依赖 Bun 模板，用于外部应用按 CortexFS ABI 发现 route/model，并通过文件 ABI 或本地 OpenAI-compatible HTTP 入口提交请求：

```bash
cd templates/bun-cortexfs-client
bun run route
bun run chat -- "Reply with exactly: cortexfs-ok"
```

默认 transport 是 `file`，适配当前实现；如已启动本地 HTTP API，可设置 `CORTEXFS_TRANSPORT=http`。

## 安全模型

CortexFS 天然按多用户设计。路径只是命名空间，不是安全边界。

访问决策基于三类身份：

- `HostActor`：Linux `uid/gid/pid`。
- `Subject`：被代表的外部用户，例如 `chat:user:123456`。
- `Object`：文件系统对象、provider、model、tool、memory、thread 等资源。

每次访问流程：

```text
FUSE request
  -> host credential
  -> optional verified external subject
  -> object context
  -> Unix mode check
  -> Cortex policy check
  -> allow/EACCES
  -> audit
```

密钥、OAuth token、session token 不进入挂载树。挂载树只暴露 secret 状态、active key id 和 rotate 控制节点，真实 secret 应由 daemon 从受保护的 secret store 解析。

## 审计和导出

全局审计：

```bash
mnt=tests/mounts/cortexfs

cat "$mnt/audit/fields"
cat "$mnt/audit/events.jsonl"
cat "$mnt/audit/usage"
cat "$mnt/audit/cost"
```

用户空间导出：

```bash
mnt=tests/mounts/cortexfs
CTX_HOME="$mnt/home/$(id -u)"
exports="$CTX_HOME/export"

cat "$exports/conversations.jsonl"
cat "$exports/sft.jsonl"
cat "$exports/preference.jsonl"
cat "$exports/tool_calls.jsonl"
cat "$exports/agent_traces.jsonl"
```

导出格式以 JSONL 为主，方便后续转成 SFT、preference、tool-call trace 和 agent trace 训练数据。`conversations.jsonl` 行包含 request、route、agent、space、subject、fingerprint 和 `time` 元数据。

过滤导出视图：

```bash
printf 'helper\n' > "$exports/filter/agent"
printf 'home/1000\n' > "$exports/filter/space"
printf '2\n' > "$exports/filter/from"
cat "$exports/conversations.jsonl"
```

当前过滤节点包括 `provider`、`model`、`agent`、`subject`、`space`、`from`、`to` 和 `exclude_failed`。`from`/`to` 使用导出行的固定宽度 `time` 值；写入普通十进制数会由运行时规范化。

## Live Test

仓库包含 ignored live tests，用本机轻量模型 fixture 验证 provider adapter 和 daemon execution plane。它们不依赖外部云 API。

当前 fixture：

```text
smollm2:135m
```

运行前先确认模型存在：

```bash
ollama list
```

如果没有 `smollm2:135m`，先拉取这个精确 fixture：

```bash
ollama pull smollm2:135m
```

运行 live tests：

```bash
cargo test -p cortex-providers --test ollama_live --locked -- --ignored --nocapture
cargo test -p cortexd --test execution_ollama_live --locked -- --ignored --nocapture
cargo test -p cortexfs --features live-tests --test ollama_file_pipeline_live --locked -- --ignored --nocapture
```

Ollama 只是当前本地 live-test fixture，不是 CortexFS 的特殊核心 provider。

一次性 daemon 执行平面 smoke：

```bash
cargo run -p cortex-cli -- daemon --once
cargo run -p cortex-cli -- daemon --once \
  --method GET \
  --endpoint /v1/models \
  --request-id models-001
cargo run -p cortex-cli -- daemon --once \
  --endpoint /v1/responses \
  --body '{"input":"hello"}' \
  --request-id resp-001
```

该命令不启动后台监听，也不实现热加载；它只验证 CLI 能把本地 API endpoint 归一化为 API format，进入 `cortexd` execution plane，并输出原生 API JSON 响应。`GET /v1/models` 作为只读发现入口从 provider model registry 派生响应，不进入 provider call 队列。

## 开发约束

- 先读 `docs/DESIGN.md`。
- 不新增 `mod.rs`。
- 新增依赖必须使用 `cargo add`。
- provider/model 设计必须保持中立。
- 开发触发事件以 Git commit 为唯一边界。
- 不新增后台监听、轮询、热加载或 `dev` 子命令。
- FUSE callback 不做远程 API 调用、长时间模型发现、向量检索、MCP 调用或 tool execution。
- 慢操作进入 daemon/execution plane。
- 挂载树是 ABI；路径、读写语义、权限语义和错误语义必须文档化并测试。

验证：

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

额外静态检查：

```bash
rg --files --glob '!vendor/**' | rg '(^|/)mod\.rs$'
rg -n "dev|watch|hot.?reload|poll|notify" README.md AGENTS.md crates .agents tests --glob '!vendor/**'
git diff --check
```

`tests/mounts/cortexfs` 是固定 FUSE 集成测试挂载点，只能作为本地挂载点使用，不要放源码、fixture 或持久化数据。
