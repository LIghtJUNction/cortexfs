# 工具、共享、策略与日志 ABI

本文件接续 [agent-tool-security.md](./agent-tool-security)。它将工具 ABI、MCP 投影规则、共享空间规则、policy v0 与日志布局与对象放置与 agent 身份、mount 设计分离。

## 工具作为文件

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
    hooks/
      pre.d/
      post.d/
  fs.list
  fs.list.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
  fs.stat
  fs.stat.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
  fs.write
  fs.write.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
  shell.exec
  shell.exec.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
```

MCP server 只是工具来源，不是 CortexFS 根对象。不应暴露：

```text
/ctx/mcp/github
/ctx/mcp/figma
```

`ctxmcp` 将显式选择的外部 stdio server 投影为普通工具。投影仅写入 v2 manifest 候选；不会安装权限、也不会授予 policy，不会写入 `/ctx`：

`ctxmcp` 宣告稳定 MCP 版本 `2025-11-25`，并仅接受协商后稳定版本 `2025-11-25`、`2025-06-18`、`2025-03-26`、`2024-11-05`。草案/未知/未来版本拒绝。

```text
/ctx/tool/github.search_issues
/ctx/tool/github.create_issue
/ctx/tool/figma.get_file
/ctx/tool/chrome.open
```

投影工具名严格为 `<server>.<remote_tool>`，且必须满足 64 字节对象名语法。没有隐式 `mcp.` 前缀或注册表。可选稳定 `mcp` 控件是严格的 stdio 定位器，包含 `transport`、可见的绝对 `config` 路径、`server` 与 `tool`。投影不会复制外部配置或 secrets。

```bash
ctxmcp list --config "$HOME/.config/example/mcp.json" --server github
ctxmcp project --config "$HOME/.config/example/mcp.json" \
  --runtime-config /workspace/.mcp.json --server github --out ./mcp-manifests
ctx object check ./mcp-manifests/github.search_issues.manifest.json
ctx object install --source /var/lib/cortexfs/storage/current \
  ./mcp-manifests/github.search_issues.manifest.json --tier system
```

工具安装仍为现有明确的 `ctx object install` 生命周期，并不会授予任何 agent 权限。

MCP-backed capabilities 可以投影为普通工具，但不作为内置默认项。CortexFS 不定义 MCP server 的配置存放位置。代理运行时或工具适配器可从代理视图内普通文件发现 MCP server。

投影出的工具控制文件仍是普通工具 ABI：

```text
/ctx/tool/github.search_issues.d/schema
/ctx/tool/github.search_issues.d/policy
/ctx/tool/github.search_issues.d/status
/ctx/tool/github.search_issues.d/log
```

实现可选地暴露诊断 origin 文件：

```text
/ctx/tool/github.search_issues.d/origin
```

`origin` 不是稳定 ABI。严格客户端不得依赖。MCP 只说明工具来源；不是新命名空间、策略类、submission 路径或 CortexFS 定义的服务端配置格式。

工具都是 executable capability endpoint：

```bash
/ctx/tool/fs.read '{"path":"README.md"}'
echo '{"cmd":"pwd"}' | /ctx/tool/shell.exec
```

可选的 agent 生命周期工具可见但仅当实现时出现：

```text
/ctx/tool/agent.create
/ctx/tool/agent.start
/ctx/tool/agent.stop
```

若 `agent.create` 存在，它必须强制衰减。至少应检查：

```text
parent agent 具有创建指定子对象的 policy permission
请求子对象权限是父权限子集
请求子对象 mount 是父可见 mount 的子集
请求子对象 groups 是父 groups 的子集
请求子对象名合法
```

授权是显式且按 child 名称粒度；参考代理不会默认继承。测试/部署 policy 里允许 `review-1` 的子权限时应显式包含：

```text
allow parent_t tool:agent.create execute
allow parent_t agent:review-1 create
allow parent_t agent:review-1 start
```

## Agent 自迭代

`/ctx/tool/agent.update` 是自迭代端点。它允许运行中的 agent 替换自己唯一一个权限无关的 control：

```text
agent/<self>.d/system.md
agent/<self>.d/prompt.template.md
```

工具通过收据绑定的 run 能力 socket 提交更新。主机将请求绑定到调用方的 agent、session、run，因此 agent 不能改写其他 agent。主机重校验 control 名称和内容，拒绝超过 8 KiB 的 payload，并在 agent 自身 control 目录原子替换。除 `system.md` 和 `prompt.template.md` 外的其他 control，包括 `policy`、`mount`、`model`、`window` 与身份文件，仍为主机所有，不可由 tool 修改，若尝试修改返回 `EINVAL`。

提示文本不授予权限。自迭代仅在下次 run 渲染 prompt 时影响行为；不会扩展 mount、policy、tools 或 Linux 身份。像任何工具调用一样，更新以普通 `tool_call`/`tool_result` 形式记录在持久会话中，因此可从会话历史审计。

执行该工具需要两层权限，和普通工具相同：

```text
allow <agent>_t tool:agent.update execute
```

在 `agent` policy 与 `tool` policy 中都必须满足。

## 代理终端工具

代理应默认仅有一个终端能力：`tsh`。运行时在 `ctxterm` 内启动它，`ctxterm` 是伪终端 owner：

```text
/ctx/bin/ctxterm
/ctx/bin/tsh
/ctx/tool/tsh
```

`ctxterm` 默认启动 `tsh` 并持有该 agent terminal 生命周期的 PTY。`ctx agent start` 在沙箱内启动该终端；默认将调用者当前目录挂载到 `/workspace` 并在此启动。`tsh` 不是宿主 shell，不会从 `PATH` 直接执行主机命令，仅通过 `CTX_PATH` 解析并执行匹配的 CortexFS tool 对象。

工具执行有两种调用方式：

```text
terminal CLI     tsh TOOL ARG...
agent native     in-process/runtime 工具调用，输入输出结构化
```

terminal CLI 模式应像普通命令行程序：保持 argv，不拦截 stdin/stdout/stderr 的继承，输出为普通文本。工具可选择认为空 argv 无效，但 `tsh` 在工具运行前不能因空 argv 拒绝普通可见工具。native mode 使用结构化 JSON 输入与 JSONL frame。可执行插件仍走与其他工具相同的授权对象路径。Tool SDK 定义了动态库 ABI，但当前核心实现不加载它；`load` 与 `pin` 目前只影响元数据上下文与缓存，不会要求 terminal CLI 发结构化 frame。

通过 `tsh` 执行工具时必须有 agent terminal context，以便 CortexFS 联合评估 agent 身份、mount、policy 与 `CTX_PATH`。独立的人类 `tsh` 进程可发现工具与查看元数据，但不能伪造 agent 身份直接执行工具。

`ctx tool NAME [ARG...]` 可以直接运行受允许的安全 CortexFS 核心 CLI（例如 `tsh.config`），这些 CLI 实现在 `ctx` 内：

```text
ctx tool NAME PATH...
```

它仍要求 `NAME` 在 `CTX_PATH` 可见，但必须拒绝普通可见工具与有权限语义的核心工具（如 `fs.write`、`shell.exec`），因为从 `CTX_PATH` 直接执行这些会绕过 agent/tool 授权链。

当终端需要观察时，其稳定定位器是会话终端 socket：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

由于 FUSE 挂载通常不能直接承载 UNIX socket，此入口统一指向 `/run/cortexfs/terminal/broker.sock`。有界 broker 协议认证对端并把已接受的描述符传给 `ctxterm`；只有 offer/prepared/accepted/commit 事务完成后才开始原始 PTY 字节流。用户级终端 socket 和旧的一行模式前缀均无效。

人类客户端应使用：

```text
ctx agent watch <agent> --session <session>
ctx agent attach <agent> --session <session>
```

`watch` 是观察优先默认；`attach` 是显式可写入加入，可能影响 agent 终端状态。

交互式 shell 与多路复用器也是普通工具：

```text
/ctx/tool/bash
/ctx/tool/tmux
/ctx/tool/zellij
```

因此代理通过 `tsh` 请求 `bash` 即可进入交互 shell。该 tool 内 `exit` 返回到 `tsh`。后台终端工作应通过可见 `tmux`/`zellij`，而不是通过第二套 agent scheduler namespace。

示例请求：

```json
{
  "name": "reviewer",
  "label": "reviewer_t",
  "model": ["openai/gpt-5.6"],
  "tools": ["fs.read"],
  "shared": {
    "project-a": ["read"]
  },
  "mount": [
    ["/work", "/work", "ro"],
    ["/shared/project-a", "/shared/project-a", "ro"]
  ]
}
```

请求成功后，工具创建普通 `agent/<name>` executable 条目、socket 条目（若支持）以及 `agent/<name>.d/*` 控制文件。不得创建新根命名空间。

代理按 `CTX_PATH` 搜索工具：

```text
/ctx/tool/fs.read
/ctx/home/1000/tool/fs.read
/ctx/shared/project-a/tool/fs.read
```

独立人类 `tsh` 会话的查找顺序为：

```text
1. CTX_HOME/.tshrc 中的 CTX_PATH=...（存在时）
2. 进程环境 CTX_PATH（若设置）
3. 默认 /ctx/tool:/ctx/home/<uid>/tool
```

`.tshrc` 不是 shell 代码。它是用户级数据文件，用于持久化工具路径配置，并优先于继承环境。

当 `tsh` 在代理终端内运行时，运行时注入的进程 `CTX_PATH` 为权威来源，因为它来自 policy、mount 与 uid/gid。

`tsh` 的持久配置保存在 `tsh` 控制目录：

```text
/ctx/tool/tsh.d/config
```

该文件是数据文件，不是 shell 代码。支持空行、`#` 注释和以下 `key=value`：

```text
max_loaded_tools=64
cache_capacity=32
window_percent=1
```

`max_loaded_tools` 限制加载入 `tsh` 上下文的未 pinned 工具元数据数量。`cache_capacity` 限制 `W-TinyLFU` 追踪的未 pinned 工具路径缓存条目。`window_percent` 配置 W-TinyLFU 的 admission window。Pinned 工具不参与自动 context unload 和路径缓存驱逐。这些配置不表示内核层加载 SDK 动态库。

持久配置通常应通过可见工具更新：

```text
/ctx/tool/tsh.config
```

上面的搜索路径描述了源分层。代理进程看到的是经过过滤的内存投影，而非原始持久目录。投影必须保留对象 ABI 形状，不得以授权副作用创建持久文件。

stdin/stdout 是主要工具接口。`schema` 为输入 JSON schema，不授予权限，也不是结果形状声明。

## 编程式工具调用

当前 ABI 内未启用编程式工具调用。CortexFS 不应宣告 OpenAI `programmatic_tool_calling` 工具、`allowed_callers`，也不应把 `schema` 当作 `output_schema`。当前仍保持 single-call host loop 作为唯一 executable-agent 工具协议。OpenAI function call 的 caller 缺失、为 null 或为 `{ "type": "direct" }` 时属于普通 native call；`program` 及未知 caller 类型必须 fail closed，视为尚不支持的 host-owned continuation。普通 direct call 执行后，下一次 OpenAI Chat/Responses 请求会重放一个规范 assistant tool call 及与其 ID 匹配的 tool result；这种协议原生重放不会启用编程式工具调用，也不会绕过 host 授权。

未来启用前，工具需要显式 default-deny 的程序化声明。可选 `tool/<name>.d/program` 是工具作者的
`readonly`/确定性声明和成功结果 JSON Schema，例如：

```json
{
  "type": "object",
  "additionalProperties": false
}
```

该声明是显式的，不可由 policy、名称或输入 schema 推断。它不授予权限，不绕过 approval，也不会使工具变成 direct-native。缺失、格式错误、非对象或 schema 校验失败会使 tool 不可用。

该 control 会被 object install、layout 检查和 bootstrap 验收，但仅保留给未来协议；在未满足前置条件前，该请求不得被执行。

### 启用约束

只有显式支持 PTC 的 provider/model route 才可接收托管 `programmatic_tool_calling` 工具。其 `allowed_callers` 只能命名通过全部门控的函数：

- 该 route 与所选 model 声明了 PTC 支持；
- `tool/<name>.d/program` 有效，其输出 schema 符合普通控制上限；
- tool 已声明、通过 `CTX_PATH` 解析，并通过正常有效权限检查；
- 当前 run 的 approval mode 为 `auto`（会触发 `Ask` 的工具不合格），且 tool 无副作用或外部写入。

如果任一门控缺失，主机必须回退普通 native 工具调用；不得仅根据工具名、policy、输入 schema 或 MCP 来源推断可用性。客户端调用面仍保持主机所有：生成程序不应持有进程、socket、filesystem 或 direct-native 权限。

启用实现必须保留 provider 返回的有界且不透明的延续事实：response identity、program item identity、每个嵌套 `call_id` 与其精确 `caller` 关联。持久审计链将这些事实与主机请求、授权决策、标准化工具结果和消费该结果的 continuation 关联，使用现有 host-owned fact 字段，不新增 CortexFS 根命名空间或稳定 event type。无状态/手工 continuation 需保留足够 bounded request 与 output 历史以复现同一 provider continuation；provider 标识和 caller 值不作为 CortexFS 权限解析。

每个生成的嵌套调用都以串行方式重新经过 declared-name、`CTX_PATH`、policy、Linux/mount、nofollow、sandbox、取消检查，并做防御性的 `Ask` 检查以处理意外 policy 变更。该检查不能把本不可用 tool 变为 PTC 可用。工具不得直接执行。

主机在返回 provider `function_call_output` 前，会按声明 program 输出 schema 校验标准化结果；只在校验通过后才发出匹配的有界 `program_output` continuation。

`program_output` 不是最终 assistant 响应：最终 assistant 消息会独立解析、授权为普通模型输出、记录并评估。

以下情况在继续或最终成功之前都应 fail closed：

| 条件 | 必要结果 |
| --- | --- |
| 模型不支持、program 控件缺失/非法、工具有副作用、或工具需要 Ask | 不要向 PTC 广告；回退普通 host loop |
| program item、`caller`、`call_id` 或 continuation 标识缺失/格式错误/溢出 | 拒绝该 provider 回合，不执行工具 |
| program 或 nested call id 重复 | 在授权前拒绝，不执行两次 |
| 在嵌套调用前/中/后取消 | 停止准备中的工作，记录取消，并不再继续 |
| 意外出现 `Ask` 要求、`Ask` 被拒、超时、EOF、响应格式错误 | 视为 PTC 不可用；记录主机侧拒绝，不返回成功 function result 或继续程序 |
| 工具失败或输出不满足 program Schema | 记录标准化失败，不发 `program_output` |
| 无效 `program_output` 或无效最终 assistant 信息 | 独立拒绝，两个 artifact 不相互通过 |

这些仅是启用前提，不代表当前 runtime、provider adapter 或 audit store 已实现完整 PTC。

执行可见性与权限由以下全部决定：

```text
Linux execute bit
agent uid/gid/groups
agent mount table 和 noexec 标记
agent policy v0 allow
tool 自有 policy
```

不存在 `agent/<name>.d/tool`。工具授权由 policy v0 决定。只有 `allow <agent_type> tool:<name> execute` 时可执行，否则 `EACCES`。

实现可只列出通过完整 effective authority 检查的 executable tool。位于 `/ctx/tool` 下的 durable tool 是候选，不是授予。用户可见性与 CortexFS security context 都约束最终可见工具集合。

MCP 来源工具使用同一策略对象类：

```text
allow executor_t tool:github.search_issues execute
allow executor_t tool:figma.get_file execute
```

工具查找严格为 `CTX_PATH`：

```text
从左到右查找同名可执行文件
命中 .d/ 时来自包含该可执行文件的目录
非可执行文件不计命中
```

## 共享

`shared/<name>` 是 Linux 权限共享空间：

```text
/ctx/shared/
  project-a/
    tool/
    data/
```

代理可见性由以下决定：

```text
agent uid/gid
agent label
mount file
共享目录权限
policy v0
```

不在此设计新的协作 DSL。高级 agent 可在 `shared/<project>` 下创建普通文件。

### 共享队列文件协议

项目队列是文件原生状态，不是 daemon 或 workflow engine：

```text
queue/
  inbox/
  pending/
  lease/
  claimed/
  done/
  failed/
```

对于名字为 `J` 的请求，且后缀 `*.req.json` 时，持久状态是：

```text
pending/J
claimed/J/J + lease/J/worker
done/J + done/J.result
failed/J + failed/J.result
```

规则：

```text
publish   写入同级临时文件，sync，然后原子 rename 为 pending/J
claim     mkdir claimed/J 为排他仲裁点；胜者将 pending/J 重命名为 claimed/J/J
lease     在创建并同步 lease/J/worker 前先同步 claim move；执行前两者都持久后才启动
finish    写临时结果并 sync，原子 rename 为 J.result，不替换，然后将请求旁路重命名
recover   未完成的 claim 在 claim 与 lease 证据都存在且无 pending/terminal 冲突时才可回到 pending
conflict  不可覆盖 pending/claimed/lease/result/terminal 证据；不完整对偶需显式对账
```

每次 rename 或移除目录后，都要 sync 所有受影响父目录。

非法名称、符号链接、非普通请求/lease 文件都不是队列任务。不得有后台 watcher、轮询服务或额外 root 路径。

`shared/cortexfs-docs` 为系统维护的 Markdown 手册束：

```text
/ctx/shared/cortexfs-docs/
  README.md
  man/
    ctx.agent.md
    ctx.tool.md
    ctx.model.md
    ctx.coreutils.md
    ctx.root-abi.md
    ctx.session.md
    ctx.provider.md
```

`/ctx/shared/cortexfs-docs/man/*.md` 是文档镜像，需与 `docs/spec/*.md` 保持对齐，避免手册长期陈旧。若安装的 `cortexfs-docs` 树陈旧，执行
`ctx bootstrap [SOURCE]` 用匹配版本树重建；若仍有陈旧内容，说明已安装 `ctx` 二进制旧版本，应升级二进制再重跑 bootstrap。

`ctx man TOPIC` 直接读取这些文件，不存在时回退到内置副本。`agent`、`model` 等话题名是 CLI 别名；持久文件名采用 `ctx.*.md` 命名空间。手册只是用户和代理的普通只读文档，不授予权限，不能扩展为第二个根 ABI 命名空间。

共享 session 是普通目录：

```text
/ctx/shared/project-a/
  agent/
    executor/
      session/
        design-review/
```

代理仅在 Linux 权限、mount 可见性与 policy v0 同时允许时可见或写入该共享 session。CortexFS 不提供 `agent/<name>.d/shared`。

## Policy v0

权限从粗到细：

```text
Linux uid/gid
file mode bit
chroot + bind mount
agent label
tool/model/agent policy
```

policy v0 是最小类型强制 allowlist。它借鉴 SELinux subject type、object class、permission 与 default deny，未复制完整 SELinux 语言。

格式：

```text
allow <subject_type> <object_class>:<object_name> <permission>
```

示例：

```text
allow executor_t tool:fs.read execute
allow executor_t tool:shell.exec execute
allow executor_t model:openai/gpt-5.6 use
allow executor_t shared:project-a read
allow executor_t shared:project-a write
allow executor_t network:default connect
allow executor_t agent:reviewer create
allow executor_t agent:reviewer start
```

规则：

```text
默认拒绝
无显式 deny
不支持 glob
无优先级
不支持继承
不支持变量展开
不支持路径匹配
未知 class 返回 EINVAL
未知 permission 返回 EINVAL
对象缺失返回 ENOENT 或 EACCES
```

固定 permission 集：

```text
tool:    execute
model:   use
shared:  read write
session: read write resume
mount:   read write
agent:   create start stop read write
network: connect
```

agent policy 使用具体名称：

```text
allow executor_t agent:reviewer create
allow executor_t agent:reviewer start
```

不要引入 glob、继承、变量或模板：

```text
allow executor_t agent:* create
```

唯一稳定网络对象名为 `default`：

```text
allow executor_t network:default connect
```

没有 `allow executor_t network:default connect`，则没有网络访问。

权限检查顺序：

```text
1. peer 凭据或 exec uid/gid
2. Linux mode bit
3. mount/chroot 可见性
4. agent label
5. object policy
6. tool/model policy
```

任一拒绝即拒绝。agent prompt、system prompt 和模型输出不能授予权限。

## 日志与事件

日志和事件不会有根级 `audit/`。日志和事件与对象共址：

```text
model/<provider>/<model>.d/log
agent/<name>.d/log
tool/<name>.d/log
home/<uid>/agent/<agent>/session/<session>/events.jsonl
shared/<name>/agent/<agent>/session/<session>/events.jsonl
```

最小事件形态：

```json
{"ts":"2026-06-22T12:00:00Z","type":"tool.call","agent":"executor","session":"default","object":"tool/fs.read","status":"ok"}
```

policy 决定是否记录敏感内容。默认日志记录事实与错误，不记录完整 secrets 或大段 prompt。
