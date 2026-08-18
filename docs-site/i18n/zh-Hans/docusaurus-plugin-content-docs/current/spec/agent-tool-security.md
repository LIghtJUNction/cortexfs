# 代理、工具与安全 ABI

层边界：

```text
model = pure inference endpoint
tool  = executable capability endpoint
agent = policy-bound orchestrator process
```

默认情况下，模型没有这些能力：

```text
tool permission
filesystem write permission
project context
long-term memory
task planning
chroot/mount policy
MCP/skill
cluster scheduling
```

Agent 拥有 Linux uid/gid/groups、label、home、root、cwd、mounts、policy、context 与工具执行决策。真实的文件写入、工具调用与共享空间访问计入 agent，而不是 model。

工具边界：

```text
model 可输出 tool_call events
model 不能执行工具
agent 决定是否执行工具
agent policy 决定执行是否允许
```

## 代理作为文件

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
    tools
    abi
    system.md
    prompt.template.md
    policy
    status
    pid
    log
    meta.json
    hooks/
      pre.d/
      post.d/
```

控制文件：

```text
owner   拥有该对象的 Linux 用户 uid
uid     runtime uid，默认等于 owner
gid     runtime gid
groups  补充组，每行一个 gid
label   CortexFS agent label，例如 user_u:agent_r:coder_t:s0
iso     隔离配置：shared、uid、userns
parent  创建该 agent 的 parent agent/session/run
life    生命周期 ownership，默认 owned
system.md 可由用户编辑的 agent 指令/persona。仅提示词文本，不具备权限语义。
prompt.template.md 可由用户编辑的系统提示模板。仅提示词文本，不具备权限语义。
abi     必需的 executable-agent 启动 ABI：sdk-envelope-v1
approval 托管 SDK direct-native 模式：auto 或 ask；缺省为 auto
```

`abi` 控制文件是必需项且仅接受 `sdk-envelope-v1`。不得从可执行内容或其他控制文件推断它。`approval=ask` 使用主机中介的交换流程。

可选的 `tools` 控制声明 agent 的静态 direct-native 工具集合。文件缺失或为空时视作空；否则每行仅一个规范工具名、末尾需换行。会拒绝空行、空白填充、重复、无效以及保留名 `tsh`。声明本身不赋予权限：

```text
direct execution = declared name AND CTX_PATH hit AND agent policy AND tool policy AND Linux/mount permission
```

每次调用会重新计算这个交集，并打开对应可执行文件（不跟随符号链接）。`tsh` 的 load/pin 缓存状态只是动态 prompt 上下文，不会扩展 direct-native 调用集合。

对于处于 `ask` 模式的 hosted SDK 代理，approval 在完成上述全部权限交集与 nofollow 打开检查后、且在进程 spawn 前进行。它是一个额外的单次调用门禁，而不是权限授予，也不代表对全部操作的等价覆盖。缺少 approval handler 的客户端应直接 fail closed。

多个 agent 可共用一个 Linux uid。uid 体现用户边界，label 体现 agent 的安全边界。

`meta.json` 可存在于较长描述（如用途、创建时间、问题编号）。策略决策不能依赖 `meta.json`。
`system.md` 是用户可编辑的 agent 身份与指令文本。
`prompt.template.md` 渲染为发送给模型的第一条系统消息。
模板支持简单变量 `{{name}}`，包括 `agent`、`current_time_unix`、`agent_instructions`、`rules`、`skills`、`tool_injection`、`history_messages`、`runtime_contract`。

渲染提示组合 `system.md`、可发现 `AGENTS.md` 规则、受限技能元数据、可选工具注入上下文、可选历史消息上下文与不可变 CortexFS 运行时约定。提示词文本不能授予工具、模型、网络、文件系统或会话权限；这些仍由 `policy`、`path`、`mount`、uid/gid 与 Linux mode bits 控制。

技能元数据仅包含 `name`、`description`、`SKILL.md` 路径。完整 `SKILL.md` 仅在技能被选中后读取。技能元数据段占用模型上下文窗口最多 2%；当上下文窗口未知时，硬上限是 8,000 字符。先缩短 description；若仍超限，则省略部分技能并包含警告。

Agent 启动流程：

```text
1. Read /ctx/agent/<name>.d/*
2. Set CTX_ROOT, CTX_HOME, and CTX_PATH
3. 合并 agent/<name>.d/env
4. 通过 uid/gid/groups/label 确定运行时身份
5. 创建 mount namespace
6. 按 mount 应用 bind mounts
7. chroot 到 root
8. cd 到 cwd
9. exec agent runtime
```

## 代理视图

代理视图是代理可见的文件、工具、模型、socket 与共享空间集合。

由以下构成：

```text
root
cwd
mount
path
model
policy
Linux uid/gid/groups/mode bits
CortexFS label
```

资源分层是独立的：

```text
/ctx/model              system models，默认对所有用户可见
/ctx/agent              system agents，默认对所有用户可见
/ctx/tool               system tools，默认对所有用户可见
/ctx/home/<uid>/model   用户模型与别名
/ctx/home/<uid>/agent   用户 agent 状态与用户代理
/ctx/home/<uid>/tool    用户工具
```

这些目录是持久资源，不等同于 agent 的运行时视图。运行时通过 `agent/<name>.d/path`、`policy`、`mount`、uid/gid 和 mode bits 在内存中投影该 agent 可见工具。为“某个系统工具对 agent 可见”而创建占位文件或符号链接副本是错误的。

CortexFS 不定义 MCP 配置格式、skill 格式、project 规则格式、或 prompt 包格式，它们都是普通文件。

示例：

```text
/home/alex/.codex/config.toml  /home/agent/.codex/config.toml  ro  bind,nosuid,nodev,noexec
/home/alex/project/.mcp.json   /work/.mcp.json                 ro  bind,nosuid,nodev,noexec
```

代理仅当文件在其 chroot 或 mount namespace 内可见，且被 Linux 权限允许时可读取。由这些文件派生的任何 capability 仍需经 CortexFS 工具策略授权执行。

技能文件是代理挂载命名空间内的普通文件。CortexFS 不定义技能文件格式。技能可见性由挂载可见性、Linux 权限与 policy 决定，技能文件本身不授予权限。

生命周期：

```text
start
ready
busy
idle
stopping
dead
```

稳定 ABI 不引入全局 daemon。建议每个 agent 进程拥有自己的 socket、pid、日志和 session。将来引入的 supervisor 属于实现细节，不应添加另一个根目录。

## 代理主目录

代理不直接使用用户 home：

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

推荐配置：

```text
/ctx/agent/coder.d/root = /ctx/home/1000/agent/coder/root
/ctx/agent/coder.d/cwd  = /workspace
```

运行时环境：

```sh
CTX_ROOT=/ctx
CTX_HOME=/ctx/home/1000
HOME=/home/agent
PATH=/usr/bin:/bin
USER=coder
LOGNAME=coder
SHELL=/usr/bin/bash
TERM=xterm-256color
LANG=C.UTF-8
```

运行时从空环境启动，仅设置上述 allowlist。默认不继承主机会话变量、桌面状态、provider secrets 与 human MCP secrets。`CTX_PATH` 在进程环境中默认不存在，除非显式授权；`tsh` 会使用 `CTX_HOME/.tshrc` 或默认工具路径。

沙箱还应掩蔽常见会重新设置环境变量的宿主 shell 启动文件，例如 `/etc/profile`、`/etc/bash.bashrc`、`/etc/profile.d`。

运行时可见 tool 目录可理解为该代理候选工具层级的过滤内存 FUSE 投影。`/ctx/tool` 中存在某工具表示系统级安装，不会自动授予任何 agent 执行权限。

## Mount 文件

`/ctx/agent/<name>.d/mount` 格式：

```text
source<TAB>target<TAB>mode<TAB>options
```

v0 解析规则：

```text
source 和 target 必须是绝对路径
source 与 target 不能包含 TAB 或换行；否则返回 EINVAL
mode 只允许 ro 或 rw
options 是用逗号分隔的小词列表
未知 option 返回 EINVAL
```

固定 v0 选项：

```text
bind
rbind
nosuid
nodev
noexec
-
```

`-` 表示无额外 option。除 `-` 外，options 不得重复。`bind` 与 `rbind` 互斥。

示例：

```text
/ctx	/ctx	ro	rbind,nosuid,nodev
/ctx/home/1000/agent/coder	/home/agent	rw	rbind,nosuid,nodev
/home/me/project	/work	rw	rbind,nosuid,nodev
/ctx/shared/project-a	/shared/project-a	rw	rbind,nosuid,nodev
/tmp	/tmp	rw	rbind,nosuid,nodev
```

## 代理创建

代理只能通过正常 CortexFS 对象与 policy 检查创建其他代理。不存在根级 `spawn/`、`factory/` 或 `agent-template/`。

`architect` 是 lineage 的普通根代理：

```text
/ctx/agent/architect
/ctx/agent/architect.sock
/ctx/agent/architect.d/
```

`architect` 不是模板命名空间，也不引入继承语义。它是普通的 agent 对象，拥有普通的 label、mount table、policy、socket、home 与 session 状态。新的顶层 agent 应通过 `agent.create` 且 `parent=agent:architect` 创建。由其他 agent 创建的子代理仍必须从直接父对象做权限衰减。

`base` 是已废弃的参考代理名。`ctx bootstrap` 报告仍存在的 `base` 对象为 `would_skip` 并保留人工复核；其 legacy 树没有可证明所有权与完整控制树完整性的清单，不应自动删除。

子代理仍是普通 agent ABI：

```text
/ctx/agent/reviewer
/ctx/agent/reviewer.sock
/ctx/agent/reviewer.d/
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
```

`parent` 是简单文本文件。通常形式：

```text
agent:architect
agent:coder
```

如需可写为：

```text
agent:coder session:default run:01H...
```

不要把 lineage 变成单独目录树。

子代理默认值：

```text
owner  = parent owner
uid    = parent uid
gid    = parent gid
groups = parent groups 子集
iso    = shared
life   = owned | temp
```

临时子代理的默认值为：

```text
life   = temp
```

每个 agent 默认应有不同 CortexFS label，除非故意处于同一安全域。重用父 label 的子代理在 policy 语义上是同一 security subject。

## 代理工具可见性

代理能见并执行的对象是交集：

```text
user-visible scope
CortexFS security context
```

用户可见范围由常规 Linux 与 mount 事实推导：

```text
agent uid/gid/groups
tool 文件 owner/group/mode bits
agent mount table
mount mode 与 noexec option
CTX_PATH 搜索顺序
```

CortexFS 安全上下文来自稳定 agent control：

```text
agent label subject，例如 coder_t
agent/<name>.d/policy
tool/<tool>.d/policy
shared/session/mount policy（相关时）
```

两侧都必须允许才可访问。可执行且挂载但未通过 policy 的工具对执行不可见；policy 允许但对该 agent uid/gid/groups 不可见或被 `noexec` 阻断的工具也不可见。提示、skills、MCP 配置文件、schemas、模型输出不会扩展该集合。

代理终端路径是：

```text
ctx agent start 启动 bwrap
bwrap 启动 ctxterm
ctxterm 启动 tsh
tsh 通过 CTX_PATH 解析工具名
人类通过 ctx agent watch 观察
人类通过 ctx agent attach 加入
```

默认 `ctx agent start` 将调用者当前目录以读写挂载到 `/workspace`，并在此启动 agent 终端。代理看到的是 sandbox 路径，不是宿主路径。其他宿主路径必须作为 sandbox 挂载声明；未挂载路径在 Linux 文件系统层不可见。

文件系统访问只有在两层都允许时才会授予：

```text
sandbox mount 暴露路径
CortexFS policy/mount 上下文授权该工具或 ABI 操作
```

建议授予 `tsh` 作为代理的主 shell 能力。`tsh` 不是宿主命令 shell，不应退回 `PATH`。`bash`、`tmux`、`zellij` 等交互行为由普通可见工具对象提供。

子代理必须衰减：

```text
子权限必须是父权限的子集
子 policy 必须是父 policy 子集，除非 supervisor 显式授予更多
子 groups 必须是父 groups 子集，除非 supervisor 显式授予更多
子 mounts 必须由父可见 mount 派生
```

mount 衰减：

```text
parent rw 可变成 child ro
parent visible 可变成 child hidden
parent ro 不能变成 child rw
parent hidden 不能变成 child visible
```

子 mount 不得暴露父不可见路径。例如父 agent 可见 `/work` 和 `/shared/project-a`，可授予子代理只读视图，但不能授予 `/home/user`、`/etc`、`/var/log` 或 `/shared/project-b`，除非 supervisor 显式授权。

当父对象死亡时，owned 子 agent 会被取消。父死亡会取消子运行时，但不应清理子会话历史。详细继承与结果处理参见 [ctx-coreutils.md](ctx-coreutils.md#core-commands)。

名称建议保持短：

```text
coder
reviewer
planner
runner
worker1
fix-123
rev-123
```

更长描述应放在 `agent/<name>.d/meta.json`。

## 工具、共享、策略与日志

Tool ABI、MCP 投影规则、共享空间访问、policy v0 和日志放置见 [tool-policy-abi.md](tool-policy-abi.md)。
