# CortexFS

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

模型、agent、tool 的名字都是文件名别名，不解析 `provider/model` 这种斜杠语义。原生 provider id、format、base URL 和模型 id 属于内部实现或 `.d/` 控制文件；用户短名用 `ln -s` 建立。

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
ctx history coder
ctx latest coder
ctx resume coder default
ctx file classify tool/fs.read
ctx file check agent/coder.d/mount
ctx validate-name coder
ctx doctor
```

`ctx history`、`ctx latest`、`ctx resume` 读取 session 文件并连接 agent socket。
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
  qwen
  qwen.sock
  qwen.d/
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
/ctx/model/qwen "hello"
echo "hello" | /ctx/model/qwen
```

`model/qwen` 是无状态模型调用入口。`model/qwen.d/` 放模型 id、driver、能力、默认参数、session 模式、状态和日志。

`model/qwen.sock` 是多轮模型会话入口。model 只负责推理，不默认拥有 tool、文件写入、项目上下文或长期记忆权限。

示例 alias：

```bash
ln -s /ctx/model/qwen /ctx/home/1000/model/coder
```

alias 只遵守 symlink 语义。不存在 `alias.d` 覆盖；需要默认参数就创建真实对象。

## tool as file

```text
/ctx/tool/
  fs.read
  fs.read.d/
    name
    description
    schema
    cap
    policy
    status
    log
  fs.write
  fs.write.d/
    name
    description
    schema
    cap
    policy
    status
    log
  shell.exec
  shell.exec.d/
    name
    description
    schema
    cap
    policy
    status
    log
```

MCP 是 tool 来源，不是新的根命名空间。不要暴露：

```text
/ctx/mcp/github
/ctx/mcp/figma
```

应该暴露普通 tool：

```text
/ctx/tool/mcp.github.search_issues
/ctx/tool/mcp.github.create_issue
/ctx/tool/mcp.figma.get_file
/ctx/tool/mcp.chrome.open
```

MCP-backed capability 可以投影成普通 tool。CortexFS 不定义 MCP server
在哪里配置，也不定义 MCP config 文件格式。agent runtime 或 tool adapter
可以从 agent view 里可见的普通文件发现 MCP server。

tool 控制文件仍然只是普通 tool ABI：

```text
/ctx/tool/mcp.github.search_issues.d/schema
/ctx/tool/mcp.github.search_issues.d/policy
/ctx/tool/mcp.github.search_issues.d/status
/ctx/tool/mcp.github.search_issues.d/log
```

实现可以提供可选诊断文件：

```text
/ctx/tool/mcp.github.search_issues.d/origin
```

`origin` 不是稳定 ABI，严格客户端不能依赖它。MCP config 是 agent 可见
世界里的普通文件，例如：

```text
/home/alex/.codex/config.toml  /home/agent/.codex/config.toml  ro  bind,nosuid,nodev,noexec
/home/alex/project/.mcp.json   /work/.mcp.json                 ro  bind,nosuid,nodev,noexec
```

最终 agent 看到的仍然是 `mcp.github.search_issues`、
`mcp.figma.get_file` 这类普通 tool。MCP 不新增 root namespace、policy
class、提交入口或 CortexFS 定义的 server 配置格式。

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

## agent as file

```text
/ctx/agent/
  coder
  coder.sock
  coder.d/
    owner
    uid
    gid
    groups
    label
    iso
    parent
    life
    root
    cwd
    env
    path
    mount
    model
    policy
    status
    pid
    log
    meta.json
```

执行：

```bash
/ctx/agent/coder
/ctx/agent/coder "修一下这个项目"
echo "继续" | /ctx/agent/coder
```

agent 启动时读取自己的 `.d/` 控制文件，按 `uid/gid/label/root/cwd/mount` 建立运行环境。TTY 下无参数执行 agent 时，默认进入交互式 socket 会话。

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

多轮交互走 `agent/<name>.sock` 或 `model/<name>.sock`。socket 请求带 `session` 和 `scope`：

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
allow coder_t model:qwen use
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
