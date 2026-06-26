---
id: developing-cortexfs
title: 二次开发
sidebar_label: 二次开发
---

# 二次开发

二次开发时先守住一个原则：CortexFS 的扩展点是当前规范里的对象、socket、控制文件和
tool 提交语义，不是新的根目录或新的 workflow 入口。

## 先读规范边界

建议顺序：

```text
DESIGN.md
spec/README.md
spec/root-abi.md
spec/object-abi.md
spec/model-abi.md
spec/session-abi.md
spec/tool-policy-abi.md
spec/ctx-coreutils.md
aimock-testing.md
```

根 ABI 只包含：

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
```

不要新增 `provider`、`workflow`、`job`、`hook`、`mcp`、`skill`、`audit` 这类顶层目录。

## 开发心智模型

CortexFS 的二次开发不从“接入一个框架”开始，而是从“操作一棵文件系统”开始。但这里的
“文件”不是传统意义上必须落在硬盘上的文件。

你在 `/ctx` 里看到的文件，可能来自硬盘，也可能是 CortexFS 按当前 agent、session、
权限和上下文即时投影出的内存视图。比如传统 agent 架构想调试上下文，往往要新增 API
接口、导出调试 JSON，或者把内部状态反复写到文件；写到硬盘会产生额外 I/O 和同步问题，
写到 tmpfs 又容易丢失。FUSE 让这些状态以文件形态出现：不 `cat`、不 `stat`、不打开，
就不需要物化；一旦需要观察，就用普通 Unix 工具读取。

这就是 CortexFS 的核心设计：把隐藏的 agent runtime 状态变成所见即所得的文件视图，同时
保留深度定制能力。agent 不需要接入一套新框架；它只要理解文件、socket、可执行 tool 和
session 这几个高层对象。

```text
写 agent/<name>.d/*     配置身份、模型、权限、挂载、工具路径
连 agent/<name>.sock    发送 JSONL 对话请求
执行 tool/<name>        运行一个受权限约束的能力
读 session/*            查看历史、事件、latest output 和 context pack
读 context/*            查看当前工作集、引用文件、child result
读 xattr/stat           判断文件类型、来源、token 估算和安全属性
```

一个最小 agent runtime 可以只是一个可执行文件：从 stdin 或 socket request 读输入，选择
`agent/<name>.d/model`，写出稳定事件帧。复杂 runtime 可以做 tool loop、上下文构建、
child agent 调度和 provider 适配，但它们仍然落在同一套对象、socket 和文件语义里。

图片、PDF、音频、压缩包等非文本输入不要塞进 prompt，也不要发明新的上传 API。推荐方式是
让文件先出现在 agent 可见的文件系统里，再在对话中引用路径：

```bash
ctx agent start coder --session default --mount "$PWD" /workspace rw
ctx send coder "请分析 /workspace/assets/screenshot.png，并对照 /workspace/docs/DESIGN.md 给建议"
```

需要跨 agent 或跨会话共享的材料，放在 shared space：

```bash
mkdir -p "$(ctx path shared project-a)/input"
cp screenshot.png "$(ctx path shared project-a)/input/"
ctx agent new reviewer --shared project-a:read
ctx send reviewer "请查看 /ctx/shared/project-a/input/screenshot.png"
```

runtime 只需要把这些路径记录进 `context/refs.jsonl` 或 context pack；真正读取图片字节、
抽取 token、生成缩略图或调用视觉模型，应由对应 tool 或 provider adapter 在需要时完成。

## 扩展 tool

tool 是可执行能力端点。用户看到的是：

```text
/ctx/tool/<name>
/ctx/tool/<name>.d/
```

具体执行可以在 Rust runner、外部程序或 runtime 内部完成，但权限仍然由 agent view、
`CTX_PATH` 和 policy 决定。

涉及异步或需要结果回收的 tool，使用统一提交语义：

```text
1. 写临时文件
2. 同目录原子 rename 成 *.req.json
3. 从 outbox 读取结果
4. 向 audit 追加事实
```

这让 tool 开发保持 Unix 风格：CLI 模式继承 argv/stdin/stdout，agent native 模式可以通过
tool SDK 走结构化 JSON 和进程内调用。两者共享同一个 `.d/schema`、`.d/policy` 和可见性
规则。

## 扩展 agent

agent 是 policy-bound orchestrator。稳定路径是：

```text
/ctx/agent/<name>
/ctx/agent/<name>.sock
/ctx/agent/<name>.d/
/ctx/home/<uid>/agent/<name>/session/
```

agent 可以组织 tool loop、上下文、child task 和 handoff，但不要把这类编排概念提升成
新的根 ABI。

### agent 树

base agent 是可继承的根身份。子 agent 的设计重点不是“复制一个进程”，而是收窄一份
可见世界：

```text
base
├── coder
│   └── reviewer
└── operator
```

父 agent 可以创建 child，但 child 的模型、tool、mount、shared space、uid/gid/groups 和
policy 必须是父级权限的子集。child 的 handoff、result、refs 和 lifecycle 都记录在父
session 的 `context/child/<id>/` 下；owned child 随父任务结束而取消，detached child
必须由 policy 明确允许。

### 终端：ctxterm 和 tsh

`ctx agent start` 的当前终端路径是：

```text
systemd-run --user
bwrap sandbox
ctxterm
tsh
```

默认把调用者当前目录挂到 sandbox 内 `/workspace`。额外挂载必须通过
`--mount SOURCE TARGET ro|rw` 显式声明；`TARGET` 不能替换 `/` 或 `/ctx`。这条路径是
agent 终端实现，不是新的后台监听、轮询或热加载子命令。

`ctxterm` 拥有 PTY，并通过 session terminal socket 暴露 `watch` 和 `attach`：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

`tsh` 只按 `CTX_PATH` 查找 tool，不回退到 host `PATH`。standalone human session
会先读 `CTX_HOME/.tshrc`，再使用继承的进程 `CTX_PATH`；该文件只能包含数据形式的
`CTX_PATH=...`。agent terminal 里的 `CTX_PATH` 由 runtime 按 policy、mount、uid/gid
生成，保持最高优先级。

这个分层很重要：

```text
ctxterm 负责 PTY 生命周期、watch/attach、多路旁观
tsh     负责 tool 发现、load/pin、按 CTX_PATH 调用能力
bash    只是一个普通 tool，只有可见且被允许时才能进入交互 shell
tmux    也是普通 tool，用来做后台任务或长期 pane
```

agent 默认 native tool 只有 `tsh`。后续工具不是凭 prompt 自动出现，而是通过 `tsh tools`、
`tsh load TOOL`、`tsh pin TOOL` 和 `tsh TOOL ARG...` 逐步进入上下文和执行路径。

### 上下文窗口管理

CortexFS 把 context 当作工作集，而不是事实源：

```text
messages.jsonl     原始对话事实，持久保存
events.jsonl       运行事件事实，持久保存
latest.md          最近输出视图，可重建
context/pack.md    当前工作集，可重建
context/refs.jsonl 被选中的文件、child result、检索结果
```

prompt 构造会合并 agent instruction、AGENTS.md 规则、skill 元数据、工具注入、历史消息和
runtime contract。Skill 只先注入 `name`、`description`、`SKILL.md path`，最多占上下文
窗口 2%；窗口未知时硬上限 8,000 字符。超限先缩短 description，再省略部分 skill 并给出
警告。完整 `SKILL.md` 只在 skill 被选中后读取。

### 权限控制

不要让 prompt 或 schema 变成权限系统。实际权限始终是多个层面的交集：

```text
mount/chroot visibility
Linux uid/gid/groups and mode bits
CortexFS label + policy v0
CTX_PATH tool visibility
tool executable metadata
noexec mount placement
```

例如 agent 能读到某个文件，不代表能执行对应 tool；能看到 tool 文件，也不代表 policy
允许执行；prompt 里写“你可以使用 shell”也不会绕过 `tsh` 和 policy。

## 扩展 provider 或本地模型

provider/model 设计必须保持中立。CortexFS 不把某个供应商写成核心默认路径，也不把
Ollama 作为核心特殊分支。

本地轻量 live test fixture 使用：

```text
smollm2:135m
```

如果该模型不存在，提示用户安装或拉取；不要静默换模型。用户明确要求测试自己配置的
供应商或聚合 API 时，走现有 provider registry、route、secret 状态和统一提交语义。

供应商 API key 的解析顺序固定为：

```text
1. root-owned CortexFS system secret store
2. 未配置，返回稳定错误或无认证请求
```

provider 配置不要声明环境变量名，用户也不需要手动配置环境变量名。API key 不注入
agent sandbox 环境；model/object runner 在请求时直接读取
`/var/lib/cortexfs/secrets/provider/<provider>/<slot>`。

OAuth access token 也按同样原则处理：运行时内部变量优先，其次系统 keychain。provider
配置可以声明 Authorization Code + PKCE 元数据；access token 默认保存在
`service=cortexfs:<provider> account=oauth:access`，refresh token 默认保存在
`account=oauth:refresh`。PKCE verifier、state、access token、refresh token 都不要写入
`/ctx/model/*`、`.d/default` 或其他 ABI 文件。

需要测试 OpenAI-compatible provider 路径而不调用云 API 时，使用本仓库的 aimock fixture：

```bash
npm install
npm run aimock
npm run aimock:smoke
```

详细说明见 [AIMock Testing](aimock-testing.md)。这是本地测试 fixture，不是新的
`/ctx/provider` 根命名空间。

多 AI API 兼容性的边界是：

```text
/ctx/model/main                    稳定默认模型 alias
/ctx/model/<provider>/<model>      provider adapter 投影出来的模型对象
model/<name>.d/driver              driver/route 元数据
provider registry/cache/keychain   runtime 内部状态，不进入根 ABI
```

换供应商时，用户改 model alias 或 route；agent 仍然只说“使用 model:main”。这样 provider
兼容性不会污染 agent、tool、session 和权限模型。

## 性能设计

CortexFS 高效的原因不是缓存堆得多，而是边界小：

```text
发现对象       目录遍历和短控制文件
执行模型/tool  文件 exec 或 Unix socket
对话 runtime   JSONL frame 流
上下文构建     原始历史持久，工作集可重建
tool 上下文    load/pin 显式进入，未 pin 项由 W-TinyLFU 回收
权限检查       静态 mount/policy/mode bit 组合
```

根 ABI 只有少量对象类，避免把 provider、数据库、workflow、MCP server 和临时任务都映射
成新目录。agent 运行时可以用内存投影加速可见 tool 列表，但 durable 状态仍然是普通文件
和稳定事件。

## 本地验证

常用检查：

```bash
cargo test
npm --prefix docs-site run build
```

FUSE 集成测试挂载点固定为：

```text
tests/mounts/cortexfs
```

该目录只作为本地测试挂载点，不放源码、fixture 或持久化数据。
