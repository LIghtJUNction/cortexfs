# CortexFS

![CortexFS turns AI runtimes into Unix-shaped files](docs/assets/cortexfs-hero.svg)

![CortexFS v1 ABI map](docs/assets/cortexfs-abi-map.svg)

![CortexFS local benchmark](docs/assets/cortexfs-performance.svg)

```bash
paru -S cortexfs-git
sudo systemctl enable --now cortexfs.service
ctx doctor
```

重新生成 README 图和本机基准：

```bash
scripts/update-readme-svg.sh
```

CortexFS 是一个面向 Linux 的 AI 文件系统 ABI 草案。当前目标不是把所有 AI 基础设施摊成复杂目录，而是保留少数 Unix 风格对象：

```text
model as file
agent as file
tool as file
```

同名 `.sock` 是有状态 socket，同名 `.d/` 目录才是控制面：

```text
name        exec endpoint
name.sock   stateful stream endpoint
name.d/     control endpoint
```

当前实现还在迁移中；新版设计以 [docs/DESIGN.md](docs/DESIGN.md) 为准。

Verus proof sources live under `proofs/verus/`. They are opt-in and do not
change the runtime Cargo workspace. Install the upstream `verus` binary from
<https://github.com/verus-lang/verus> and run:

```bash
scripts/verify-verus.sh
```

Current proofs cover the v1 object-name ABI predicate; see
[docs/proofs/verus.md](docs/proofs/verus.md).

`/ctx/model` 使用 `provider/model` 两层命名，例如
`openai/gpt-4o`、`anthropic/claude-sonnet-4`、`google/gemini-2.5-pro`。
原生模型使用原始 provider；自定义 base URL 没有原始 provider 映射时，
provider 自动取规范化域名，例如 `https://api.lmm.best:9000/` 投影为
`api.lmm.best/<model>`。短名或默认选择用 `ln -s` 建立。agent 和 tool
仍保持单段对象名。

`/ctx/model/main` 是约定默认模型别名，默认符号链接到
`/ctx/model/debug/echo`；`/ctx/model/helper` 是帮主模型处理杂活的约定
别名，默认也指向 `debug/echo`。切换模型时改这些别名即可。

资源层级固定分开：

```text
/ctx/model              系统模型，默认所有用户可见
/ctx/agent              系统 agent，默认所有用户可见
/ctx/tool               系统工具，默认所有用户可见
/ctx/home/<uid>/model   用户自己的模型和 alias
/ctx/home/<uid>/agent   用户自己的 agent 状态和用户 agent
/ctx/home/<uid>/tool    用户自己的工具
```

agent 运行时看到的 tool 集合不是这些目录的落盘副本。它由
`agent/<name>.d/path`、policy、mount、uid/gid 和 mode bits 计算，然后通过
FUSE 在内存里投影给该 agent。不要为了表示“agent 可见 fs.read”就在
`/ctx/home/<uid>/tool` 里放一个默认 symlink；`/ctx/tool/fs.read` 是系统
工具，是否可执行由 runtime view 和 policy 决定。

CortexFS 不重新实现 AI API 兼容层。OpenAI、Anthropic、Google、本地模型和聚合 API 的兼容交给 Rig；CortexFS 只做更高层的 Agent OS 文件 ABI、会话、权限、mount、shared/home 和 tool 查找。

## 根目录草案

```text
/ctx/
  status
  bin/
  model/
  agent/
  tool/
  home/
  shared/
```

不再把 provider、format、db、vector、audit、cluster、mcp、skill 等作为根目录暴露。它们可以是内部实现、agent 能力、tool 能力或 agent 可见的普通文件，但不是用户必须理解的根 ABI。

`AGENTS.rc` 不属于稳定 ABI。配置应该是数据，不应该是自动执行的 shell 脚本；严格客户端只依赖 `agent/<name>.d/env`、`path`、`mount` 等数据文件。

Agent view 是 agent 能看见的文件、tool、model、socket 和 shared space。
它由 `root`、`cwd`、`mount`、`path`、`model`、`policy`、Linux
uid/gid/groups/mode bits 和 CortexFS label 派生。CortexFS 管“看见什么、
执行什么、共享什么”，不定义 MCP config、skill、project rule 或 prompt
package 的文件格式；这些都是 agent mount namespace 里的普通文件。skill
不授予权限，任何派生能力的执行仍然必须走 `/ctx/tool`、`CTX_PATH` 和 policy。

## Linux 风格约定

```text
对象发现      readdir /ctx/model、/ctx/agent、/ctx/tool
控制面        只写同名 .d/ 小文件
交互数据面    只走 .sock JSONL
exec 错误     exit code + JSONL error frame
socket 取消   SIGINT 先发送 {"op":"cancel"}
agent 生命周期 普通进程拥有自己的 pid、sock、log、session
policy v0     type-enforcement allowlist，default deny
```

第一版不引入全局 daemon。未来如果需要 supervisor，它也只是实现细节，不增加根目录。

## ctx CLI

正式命令行工具名是 `ctx`。第一版只做 CortexFS 文件系统管理，不做 provider、daemon、挂载或私有 session 存储：

```bash
ctx status
ctx abi
ctx env
ctx root
ctx bootstrap
ctx mount
ctx ls
ctx ls model
ctx ls agent
ctx ls tool
ctx which tool fs.read
ctx path shared project-a
ctx agent history coder
ctx agent output coder
ctx agent resume coder --session default
ctx file classify tool/fs.read
ctx file check agent/coder.d/mount
ctx validate-name coder
ctx doctor
```

`ctx agent history`、`ctx agent output`、`ctx agent resume` 读取 session 文件并连接 agent socket。
这些命令不传 `--session` 时会使用 `session/index/current`，不存在时退回 `default`。
后续可以把 `ctx chat`、`ctx sessions` 做成 agent socket 薄客户端，但不能维护 provider registry 或私有多轮状态。

## 环境变量

```sh
export CTX_ROOT=/ctx
export CTX_HOME="$CTX_ROOT/home/$(id -u)"
export CTX_PATH="$CTX_ROOT/tool:$CTX_HOME/tool"
export PATH="$CTX_ROOT/bin:$PATH"
```

含义：

```text
CTX_ROOT  CortexFS 挂载根
CTX_HOME  当前 Linux 用户的 CortexFS home
CTX_PATH  agent 查找 tool 的路径，类似 PATH
PATH      普通 shell 命令路径
```

## model as file

```text
/ctx/model/
  debug/
    echo
    echo.d/
      id
      driver
      cap
      default
      session
      status
      log
  openai/
    gpt-4o
    gpt-4o.d/
      id
      driver
      cap
      default
      session
      status
      log
```

执行：

```bash
/ctx/model/debug/echo "hello"
echo "hello" | /ctx/model/openai/gpt-4o
```

`model/debug/echo` 是最小调试模型：无状态、无 provider、无默认云模型，只把输入作为 JSONL delta 回显。`model/debug/echo.d/` 放模型 id、driver、能力、默认参数、session 模式、状态和日志。

`model/<provider>/<model>` 是只读可执行对象：读取它看到的是模型元数据；执行时由 CortexFS/Rust runtime 或 provider adapter 处理。API key 不写入模型文件或 `.d/`，解析顺序固定为环境变量、系统 keychain、未配置。

`tool/<name>` 同样是只读可执行元数据入口。读取工具文件看到的是
`#!/usr/bin/cortexfs-object-runner` 开头的 CortexFS metadata，而不是每个
工具自己的 shell 脚本；具体工具分发在 Rust runner 后面完成。

读正文前可以先读扩展属性做成本判断：

```bash
getfattr -d /ctx/tool/fs.read
getfattr -n user.cortexfs.token_estimate /ctx/model/main
getfattr -n user.cortexfs.origin /ctx/model/helper
```

`user.cortexfs.origin` 区分 `virtual`、`disk`、`overlay`；`storage` 区分
`memory` 和 `disk`；`token_estimate`、`input_token_estimate`、
`output_token_estimate`、`cache_bytes`、`cache_entries` 用于让 agent 在读取前
判断上下文成本和缓存状态。

自定义 base URL 的持久配置放在 `/etc/cortexfs/providers.d/*.json`，文件
只保存非密钥配置：

```json
{
  "base_url": "https://api.lmm.best:9000/",
  "default_model": "gpt-5.4-mini",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
```

基础元数据字段对齐 Rig 0.39 `ModelListingClient::list_models()` 返回的 `Model`：`id`、`name`、`description`、`type`、`created_at`、`owned_by`、`context_length`。

`model/<provider>/<model>.d/driver` 是 driver 路由表。旧的单行值仍可用：

```text
debug
```

新格式按调用场景选择 driver：

```text
default=openai-chat
exec=openai-chat
socket=openai-chat
agent=openai-responses,openai-chat
```

含义：

```text
exec    直接执行模型文件
socket  直接连接 model socket
agent   agent runtime 调模型
default fallback
```

所以同一个 `/ctx/model/openai/gpt-4o` 可以在直接对话时走经典 chat driver，
在 agent 调用时优先走 Responses-style driver，并在不可用时回退到 chat。
driver 是 adapter 选择，不改变稳定模型名。

只有 `*.d/session` 为 `socket` 的模型才暴露 `model/<provider>/<model>.sock`。model 只负责推理，不默认拥有 tool、文件写入、项目上下文或长期记忆权限。

稳定模型名始终使用原始提供者，而不是聚合商名：

```text
openai/gpt-4o
openai/gpt-4.1
anthropic/claude-sonnet-4
google/gemini-2.5-pro
meta-llama/llama-4-maverick
x-ai/grok-4
```

如果后端接入的是 `lmm.best` 这类聚合 API，CortexFS 仍暴露
`/ctx/model/openai/gpt-4o` 这样的原始模型名；聚合商只作为 driver/source
配置或链接目标存在。

示例 alias：

```bash
ln -s /ctx/model/openai/gpt-4o /ctx/home/1000/model/main
ln -s /ctx/model/debug/echo /ctx/home/1000/model/coder
```

alias 只遵守 symlink 语义。不存在 `alias.d` 覆盖；需要默认参数就创建真实对象。

## tool as file

```text
/ctx/tool/
  fs.read
  fs.write
  shell.exec
```

每个 tool 都是可执行 capability endpoint，同名 `.d/` 保存 `name`、
`description`、`schema`、`cap`、`policy`、`status` 和 `log` 等控制文件。
MCP 是 tool 来源，不是新的根命名空间。不要暴露：

```text
/ctx/mcp/github
/ctx/mcp/figma
```

如果 MCP adapter 已经真实接入并生成了完整 schema，可以暴露普通 tool，
但它仍然走普通 tool ABI 和 policy：

```text
/ctx/tool/mcp.github.search_issues
/ctx/tool/mcp.github.create_issue
/ctx/tool/mcp.figma.get_file
/ctx/tool/mcp.chrome.open
```

MCP config 是 agent view 里的普通文件；MCP 不新增 root namespace、
policy class、提交入口或 CortexFS 定义的 server 配置格式。

执行：

```bash
/ctx/tool/fs.read '{"path":"README.md"}'
```

agent 调 tool 时按 `CTX_PATH` 搜索同名可执行文件：

```text
/ctx/tool/fs.read
/ctx/home/1000/tool/fs.read
/ctx/shared/project-a/tool/fs.read
```

这些是候选来源层。实际 agent 进程可以看到的是过滤后的内存 FUSE 视图，
不是把系统工具复制或链接到用户目录后形成的持久目录。

## agent as file

```text
/ctx/agent/
  coder
  coder.sock
  coder.d/
```

执行：

```bash
/ctx/agent/coder
/ctx/agent/coder "修一下这个项目"
echo "继续" | /ctx/agent/coder
```

agent 启动时读取自己的 `.d/` 控制文件，按 `owner`、`uid/gid/groups`、
`label`、`root/cwd/mount`、`model`、`path` 和 `policy` 建立运行环境。
TTY 下无参数执行 agent 时，默认进入交互式 socket 会话。

`agent/coder.sock` 是交互式 agent 会话入口。agent 是高层对象：它可以在 policy 允许下使用 model、tool 和 shared。

层级边界：

```text
model = pure inference endpoint
tool  = executable capability endpoint
agent = policy-bound orchestrator process
```

## agent home

每个 agent 有自己的私有 home：

```text
/ctx/home/1000/
  agent/
    coder/
      root/
      session/
      data/
      cache/
      log/
  tool/
  model/
```

推荐：

```text
/ctx/agent/coder.d/root = /ctx/home/1000/agent/coder/root
/ctx/agent/coder.d/cwd  = /work
```

## mount

`agent/<name>.d/mount` 是 bind mount 表：

```text
source<TAB>target<TAB>mode<TAB>options
```

示例：

```text
/ctx	/ctx	ro	rbind,nosuid,nodev
/ctx/home/1000/agent/coder	/home/agent	rw	rbind,nosuid,nodev
/home/me/project	/work	rw	rbind,nosuid,nodev
/ctx/shared/project-a	/shared/project-a	rw	rbind,nosuid,nodev
/tmp	/tmp	rw	rbind,nosuid,nodev
```

新增挂载目录就是追加一行。辅助命令可以放在 `/ctx/bin`，但 ABI 是这个 `mount` 文件。

## session / resume

多轮交互走 `agent/<name>.sock` 或 `model/<provider>/<model>.sock`。socket 请求带 `session` 和 `scope`：

```jsonl
{"op":"send","id":"client-msg-id","session":"default","scope":"private","cwd":"/work","input":"你好"}
```

scope：

```text
private  当前 uid 私有，可 resume
shared   写入 /ctx/shared/<name>，按权限共享
temp     临时会话，不要求恢复
```

agent 私有会话存放在：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/
```

客户端实现 resume 时读取 `session/index/list`、`session/index/current` 和
`session/index/by-cwd/*`，不要自己维护一套不可见历史库。

## Policy v0

Policy v0 是最小 type-enforcement allowlist：

```text
allow coder_t tool:fs.read execute
allow coder_t tool:shell.exec execute
allow coder_t model:openai/gpt-4o use
allow coder_t shared:project-a read
allow coder_t shared:project-a write
```

规则：default deny，没有 explicit deny、glob、优先级、继承、变量展开或 path matching。agent label `user_u:agent_r:coder_t:s0` 中用于匹配的是 `coder_t`。

## 权限模型

权限从粗到细：

```text
Linux uid/gid
文件 mode bit
chroot + bind mount
agent label
tool/model/agent policy
```

agent 的 label 类似 SELinux：

```text
user_u:agent_r:coder_t:s0
```

目标是让 agent 很难靠提示词或路径扫描逃逸，而不是把所有安全逻辑写进一个 shell 脚本。

## 开发

旧 CLI、daemon、provider registry、FUSE 投影和旧 docs-site 已删除。当前仓库只保留新版 ABI 设计和一个可编译的最小 Rust crate；后续按设计从零重写。

```bash
cargo build --locked --workspace
cargo test --locked --workspace
```
