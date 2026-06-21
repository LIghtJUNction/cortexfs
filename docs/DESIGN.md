# CortexFS 设计规范

CortexFS 是一个面向 AI API、Agent、工具调用、MCP、Skills、记忆和集群协作的多用户 FUSE 文件系统。

它遵循“万物皆文件”的 Unix 设计：模型、provider、API format、请求、响应、线程、工具、MCP server、skill、agent、任务、记忆、审计和控制面都被暴露为稳定的文件系统对象。文件系统是 ABI，不是 UI；每个暴露节点都必须有明确的读写语义、权限语义和错误语义。

`cortexfs` 只做 FUSE/VFS 投影。`cortexd` 负责执行：网络 API 调用、provider 路由、MCP 通信、tool loop、skill 解析、权限决策、审计、缓存、记忆检索、训练数据导出和集群调度。

设计必须克制：不要因为上层产品里有“channel”“job”“hook”“workflow”等名词，就把它们变成新的 ABI 目录。CortexFS 的核心提交语义是统一的文件队列：写临时文件、同目录原子 rename 成 `*.req.json`、从 outbox 读结果、向 audit 追加事实。

## 1. 基本原则

- FUSE 回调不做远程 API 调用。
- FUSE 回调不做长时间模型发现、向量检索、MCP 调用或 tool execution。
- 所有慢操作进入 daemon 队列。
- 普通 `write()` 只修改文件内容；只有同目录原子 `rename` 到 `*.req.json` 才表示提交。
- 所有提交入口共享同一条 staging、rename、queue、outbox、audit 语义。
- provider/model 设计必须保持中立；测试 fixture 不能变成核心默认路径、核心能力或特殊分支。
- 文件路径是命名空间，不是安全边界。
- 安全决策基于 host credential、external subject、object context 和 policy。
- 小配置使用小文本属性文件。
- API request/response 使用原生 JSON。
- 对话历史使用 JSONL。
- socket 只作为实时 fast path，不是 source of truth。
- 所有路径、文件名和格式一旦稳定就是 ABI。
- 开发期刷新以 Git commit 作为唯一事件边界。
- 不提供后台监听、轮询或热加载子命令。
- 已挂载实例暴露一个确定的实现版本；新版本通过提交后的重新构建和重新挂载进入运行态。

## 2. 顶层目录

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

含义：

```text
status        全局状态
cap/          全局能力列表
format/       API 协议格式
provider/     后端提供商和账号实例
model/        全局模型索引
home/         类 /home 的用户入口，home/<uid> 指向用户 space
group/        本机组入口
shared/       共享项目/协作入口
ext/          外部平台入口
space/        策略视图
agent/        agent 定义、运行时和协作入口
cluster/      agent/worker 集群
mcp/          MCP server、tools、resources、prompts
skill/        skill registry 和 skill 内容投影
tool/         Cortex 原生工具和外部工具投影
memory/       全局记忆和索引入口
vector/       向量数据库后端
db/           PostgreSQL/SQLite 等结构化后端
audit/        全局审计视图
control/      全局控制节点
```

开发期只保留当前 ABI。ABI 只暴露上面的单数、短名词目录；同一对象不得再暴露第二套入口或复数形式。挂载树不是可扩展数据目录；对未声明 ABI 目录执行 `mkdir` 必须返回 EROFS，不能在运行态动态创建新的顶层目录或用户子入口。

明确不提供这些入口：

```text
chan/              provider/route 的第二套抽象
home/<uid>/job/    上层任务 DSL
home/<uid>/hook/   上层触发器 DSL
workflow/          上层编排 DSL
```

如果外部系统需要“任务”“触发器”“步骤”，应把它们写入请求 JSON、thread metadata、external subject 或自己的状态库，然后通过通用 inbox/outbox 提交。

## 3. 文件类型约定

```text
无扩展名        小文本属性或控制节点
*.req.json      原生 API 请求
*.resp.json     原生 API 响应
*.error         错误对象
*.jsonl         append-only 日志、消息、审计、训练数据
*.md            人类可读视图
*.sock          Unix domain socket fast path
schema.json     大结构 schema
manifest.json   大结构 manifest
```

小文本属性规则：

```text
一个文件一个值
多值一行一个
布尔值 0/1
整数为十进制
读取带结尾换行
非法写入返回 EINVAL
无权限返回 EACCES
只读写入返回 EROFS
不支持返回 ENOSYS
```

## 4. API Format

API format 是请求协议形状，不等于 provider。

```text
format/
  openai.chat/
    name
    schema.json
    request_suffix
    response_suffix
    content_type
  openai.responses/
  anthropic.messages/
  google.generate_content/
```

使用 OpenAI 请求格式的 provider 共享 `openai.chat` 或 `openai.responses`。使用 Anthropic 请求格式的 provider 共享 `anthropic.messages`。Google/Gemini 使用 `google.generate_content`。

## 5. Provider 与 Base URL

Provider 是后端实例，不是域名，也不是厂商品牌。一个厂商可以有多个 provider instance；一个中转站也可以是 provider。

```text
provider/
  inbox/
  outbox/
  openai-main/
  openai-relay-a/
  kimi-main/
  minimax-main/
  deepseek-main/
  codex-account/
  local-vllm/
```

Provider 结构：

```text
provider/<id>/
  family
  name
  format
  url/
    default
    current
    effective
    source
  auth
  acct
  enabled/
    default
    current
    effective
    source
  priority/
  health/
    status
    latency_ms
    last_error
    check
  secrets/
    status
    active
    rotate
    last_rotated
    next_rotation
    inbox/
    outbox/
  model/
    count
    refresh
    <model-id>/
      name
      format
      context_window
      max_output_tokens
      cap
      status
```

`acct`：

```text
key
oauth
session
service_account
local_runtime
```

密钥、OAuth token、session token 不进入挂载树，只暴露状态和 key id。

新增或更新 provider instance 使用统一提交语义：向 `provider/inbox/` 写临时 JSON 文件，再同目录原子 rename 成 `*.req.json`。实现把配置保存到本机 provider registry；不得把 API key 放入该 JSON。

```json
{
  "op": "upsert",
  "id": "openai-relay-a",
  "family": "openai-compatible",
  "name": "OpenAI-compatible relay A",
  "formats": ["openai.chat", "openai.responses"],
  "base_url": "https://relay.example.com/",
  "default_model": "gpt-4o-mini",
  "priority": 80,
  "enabled": true
}
```

Provider secret import 也使用同一条提交语义，但入口在 `provider/<id>/secrets/inbox/`。明文 secret 只允许出现在提交请求体内，处理后必须进入系统 secret store；挂载树只暴露 `status` 和 `active` secret reference。

```json
{
  "op": "import",
  "kind": "bearer",
  "value": "sk-placeholder"
}
```

## 6. 模型视图

全局模型索引：

```text
model/
  count
  <provider-id>.<model-id>/
    provider
    model
    format
    cap
    status
```

Provider 原始模型：

```text
provider/<id>/model/count
provider/<id>/model/<model-id>/
```

用户实际可用模型：

```text
home/1000/model/
  count
  refresh
  <provider-id>.<model-id>/
    provider
    model
    format
    allowed
    reason
    cap
```

查询“某个用户/agent 能用多少模型”时读取用户模型视图，而不是 provider 全局模型视图。

## 7. Space

Space 是权限、审计、记忆和执行的边界。

```text
space/
  count
  list
  uid1000/
    context
    entry
    kind
  shared.project-a/
    context
    entry
    kind
  ext.<platform>/
    context
    entry
    kind
```

`space/` 是只读安全上下文索引，不是第二个可操作入口。不要在 `space/` 下复制 `api/`、`thread/`、`memory/`、`export/` 等用户工作树。

推荐 ABI 入口使用短名词：

```text
home/<uid>          用户入口
group/<gid>         组入口
shared/<name>       共享协作入口
ext/<platform>/...  外部平台入口
```

`space/` 是策略视图。日常脚本应使用 `home/<uid>`、`group/<gid>`、`shared/<name>`、`ext/<platform>/...` 这些直接入口；开发期不提供 `spaces/` 目录，也不提供 `space/users/<uid>` 这类第二入口。

为方便 shell 脚本和 agent 软件使用，挂载根提供类 `/home` 的用户入口：

```text
home/<uid>
```

推荐实机环境变量：

```bash
export CTX_HOME=/ctx/home/$(id -u)
```

`CTX_HOME` 是当前 Linux 用户的 CortexFS 工作入口，包含该用户的 `api/`、`thread/`、`memory/`、`export/`、`tool/`、`mcp/`、`skill/` 等视图。

写入 `$CTX_HOME/api/.../inbox` 必须进入 user outbox 和审计流。UID 是 ABI 身份名；用户名只属于上层展示，不作为安全边界。`space/uid1000/entry` 只读指向 `home/1000`，不能作为提交入口。

每个 space：

```text
policy/
route/
model/
api/
thread/
agent/
tool/
mcp/
skill/
memory/
cache/
audit/
export/
convert/
control/
```

## 8. 原生 API 文件接口

每个用户工作入口按 format 暴露 API：

```text
home/1000/api/
  openai.chat/
    inbox/
    outbox/
  openai.responses/
    inbox/
    outbox/
  anthropic.messages/
    inbox/
    outbox/
  google.generate_content/
    inbox/
    outbox/
```

提交：

```bash
mv req.tmp /mnt/cortex/home/1000/api/openai.chat/inbox/001.req.json
```

响应：

```text
outbox/001.resp.json
outbox/001.error
```

规则：

- `write()` 不触发 API。
- 同目录 rename 到 `inbox/*.req.json` 才触发提交。
- rename 只负责入队、计算 fingerprint、记录 route metadata 和 audit；FUSE 回调不得在提交路径里调用远程 provider。
- request id 是幂等 key。
- 请求和响应保持原生 API JSON。
- 每次请求都写 audit。
- 每次请求都计算 fingerprint。

## 9. 统一本地 API

CortexFS 同时提供本地 API：

```text
127.0.0.1:6185
/run/user/<uid>/cortex/api.sock
```

HTTP endpoint：

```text
GET  /v1/models
POST /v1/chat/completions
POST /v1/responses
POST /v1/messages
POST /v1/generateContent
```

本地 API 与 FUSE 文件路径必须进入同一内部管线：

```text
normalize format
route
policy check
secret resolve
provider call
store response
append thread if bound
audit
```

不能存在不审计、不受 policy 控制、不写 store 的旁路。

## 10. Thread 与持续通信

Thread 是持续上下文。

```text
thread/<id>/
  inbox/
  io.sock
  messages.jsonl
  latest.md
  state
  fingerprint
  control/
```

`messages.jsonl`：

```jsonl
{"role":"system","content":"..."}
{"role":"user","content":"..."}
{"role":"assistant","content":"..."}
```

文件式提交路径适合批处理、离线导入和不需要流式响应的调用：

```bash
mv msg.tmp thread/demo/inbox/0001.req.json
```

交互式 agent/chat/REPL 客户端必须优先使用 socket fast path，避免每一轮对话写临时文件再 rename：

```text
thread/demo/io.sock
```

Socket 协议使用 JSONL：

```json
{"op":"send","message":{"role":"user","content":"继续"}}
```

返回：

```jsonl
{"type":"accepted","request_id":"001"}
{"type":"delta","content":"可以"}
{"type":"message","role":"assistant","content":"可以这样..."}
{"type":"done","request_id":"001"}
```

Socket 必须：

- 校验 `SO_PEERCRED`。
- 走同一 policy。
- 写同一 store。
- 更新 `messages.jsonl`、`latest.md`、`state`、`fingerprint`。
- 写 audit。

文件管事实和批处理提交，socket 管实时交互。socket 不是旁路：runtime 必须把 socket turn 写回 `messages.jsonl`、`latest.md`、`state`、`fingerprint` 和 audit。

## 11. 批处理

批处理是文件系统的一等能力。

```text
home/1000/batch/
  inbox/
  outbox/
  state
  count
```

提交：

```bash
for f in requests/*.json; do
  mv "$f" "/mnt/cortex/home/1000/batch/inbox/$(basename "$f" .json).req.json"
done
```

读取：

```bash
for f in /mnt/cortex/home/1000/batch/outbox/*.resp.json; do
  jq -r '.choices[0].message.content' "$f"
done
```

批处理要求：

- 并发安全。
- request id 幂等。
- 支持 `find`、`xargs`、`parallel`。
- 每个请求有独立 error。
- 每个请求可审计、可导出训练数据。

## 12. MCP

MCP 是一等文件系统对象。

```text
mcp/
  server/
  tool/
  resource/
  prompt/
  session/
```

MCP server：

```text
mcp/server/<server-id>/
  name
  transport
  command
  args
  url
  env/
  status
  pid
  cap
  control/
    start
    stop
    restart
```

`transport`：

```text
stdio
sse
http
websocket
```

MCP tool 投影：

```text
mcp/tool/<server-id>.<tool-name>/
  name
  description
  input_schema.json
  output_schema.json
  invoke/
    inbox/
    outbox/
  permissions
```

调用 MCP tool：

```bash
mv call.tmp mcp/tool/github.create_issue/invoke/inbox/001.req.json
cat mcp/tool/github.create_issue/invoke/outbox/001.resp.json
```

MCP resources：

```text
mcp/resource/<server-id>/<resource-id>/
  uri
  mime_type
  content
  refresh
```

MCP prompts：

```text
mcp/prompt/<server-id>/<prompt-id>/
  name
  arguments_schema.json
  render/
    inbox/
    outbox/
```

MCP session：

```text
mcp/session/<session-id>/
  server
  state
  transcript.jsonl
  io.sock
```

MCP 调用必须走 Cortex policy 和 audit。MCP server 不能绕过 provider、tool、secret、space 权限。

## 13. Tools 与 Tool Loop

Tool 是一等执行对象。MCP tool、shell tool、本地函数、provider tool 都统一投影到 `tool/`。

```text
tool/
  shell.exec/
  filesystem.read/
  mcp.github.create_issue/
  provider.openai.web_search/
```

Tool 目录：

```text
tool/<tool-id>/
  name
  description
  kind
  input_schema.json
  output_schema.json
  permissions
  invoke/
    inbox/
    outbox/
```

Tool loop 是 thread/agent 下的 append-only 执行链：

```text
thread/<id>/tool-loop/
  state
  steps.jsonl
  control/
    continue
    pause
    cancel
```

`steps.jsonl`：

```jsonl
{"step":1,"type":"model","message":"..."}
{"step":2,"type":"tool_call","tool":"mcp.github.search","input":{...}}
{"step":3,"type":"tool_result","tool":"mcp.github.search","output":{...}}
{"step":4,"type":"model","message":"..."}
```

Tool loop 要求：

- 每步可审计。
- 每个 tool call 有 permission check。
- 可暂停、恢复、取消。
- 有最大步数、最大时间、最大成本限制。
- tool result 进入 thread history 或 artifacts。

## 14. Skills

Skill 是一等知识和工作流对象。

```text
skill/
  registry/
  installed/
  index/
```

Skill 目录：

```text
skill/installed/<skill-id>/
  name
  description
  version
  triggers
  SKILL.md
  references/
  scripts/
  assets/
  examples/
  permissions
  status
```

Skill 的 progressive disclosure 通过文件系统自然表达：

```text
name/description/triggers  常驻小属性
SKILL.md                   触发后读取
references/                按需读取
scripts/                   可执行资源
assets/                    输出资源
```

Agent 可以读取：

```text
skill/index/by-trigger/
skill/index/by-domain/
```

Skill 可以声明需要的 tool、MCP servers、provider：

```text
skill/installed/<skill-id>/permissions
```

Skill 不能自动获得权限；必须由 space/agent policy 授权。

## 15. Agent

Agent 是一个带 profile、policy、tool、skill、memory 和 thread 的执行主体。

```text
agent/<agent-id>/
  profile/
  runtime/
  policy/
  skill/
  tool/
  mcp/
  memory/
  thread/
  inbox/
  outbox/
  io.sock
```

Profile：

```text
profile/name
profile/description
profile/system_prompt
profile/model/
profile/style
```

Runtime：

```text
runtime/state
runtime/pid
runtime/heartbeat
runtime/current_thread
runtime/current_task
```

Agent 可以通过：

```text
inbox/outbox  文件式任务
io.sock       实时交互
thread/       长期上下文
skill/       工作流能力
mcp/          外部工具能力
memory/       记忆范围
```

## 16. Agent 协作

Agent 协作通过共享 task、thread、blackboard 和 handoff 文件实现。

```text
shared/<project>/collab/
  blackboard/
  task/
  handoff/
  lock/
  decision/
```

Blackboard：

```text
blackboard/
  notes.jsonl
  state
  artifact/
```

Task：

```text
task/<task-id>/
  spec.md
  owner
  state
  claim/
  events.jsonl
  result/
```

Handoff：

```text
handoff/<handoff-id>/
  from
  to
  summary.md
  context_refs
  state
```

协作规则：

- 任务领取通过 atomic create/rename。
- 协作事件写 `events.jsonl`。
- 锁必须有 lease/timeout。
- 所有 agent 操作写 audit。

## 17. Agent 集群

集群组织多个 agent 和 worker。

```text
cluster/<cluster-id>/
  agent/
  worker/
  queue/
  task/
  scheduler/
  state
  policy/
  audit/
```

Worker：

```text
worker/<worker-id>/
  state
  heartbeat
  cap
  load
  current_task
```

Queue：

```text
queue/default/
  pending/
  running/
  done/
  failed/
```

Task：

```text
task/<task-id>/
  spec.req.json
  state
  assigned_worker
  result.resp.json
  error
  audit
```

调度规则：

- Worker 根据 cap 和 policy 领取 task。
- Task claim 必须原子。
- Worker 崩溃后 task 可重试。
- 成本、步数、时间受 policy 控制。
- 集群不绕过 space 权限。

## 18. 分层记忆

记忆分层：

```text
memory/
  working/
  episodic/
  semantic/
  procedural/
  profile/
  index/
```

含义：

```text
working      当前任务工作集
episodic     对话事件和经历
semantic     抽象事实和知识
procedural   工具使用方法和工作流
profile      用户、agent、persona 长期属性
```

Space 记忆：

```text
home/1000/memory/
  working/
    inbox/
    items.jsonl
  episodic/
    inbox/
    items.jsonl
  semantic/
    inbox/
    items.jsonl
  procedural/
    inbox/
    items.jsonl
  profile/
    inbox/
    items.jsonl
  search/
  policy/
```

Thread 引用记忆范围：

```text
thread/demo/memory_scope
```

内容：

```text
home/1000:semantic
home/1000:profile
shared/project-a:procedural
```

## 19. 向量数据库

向量数据库是 memory/index 后端。

```text
vector/
  store/
    local/
    pgvector/
    qdrant/
    milvus/
  index/
```

Vector store：

```text
vector/store/pgvector/
  enabled/
  status
  dimension
  distance
  collections
  refresh
```

Memory search：

```text
home/1000/memory/search/
  query
  results.jsonl
```

`query` 可写，`results.jsonl` 是只读派生视图。

## 20. 数据库

结构化后端：

```text
db/
  sqlite/
  postgres/
```

Postgres：

```text
db/postgres/
  status
  dsn/
    default
    current
    effective
    source
  migration/
  pool/
```

DSN 不暴露密码。密码来自 secret store。

PG 用途：

```text
thread metadata
audit events
usage/cost
route/policy snapshots
memory metadata
pgvector
task queue metadata
```

## 21. 审计

审计是一等能力。

```text
audit/
  events.jsonl
  usage
  cost
```

每条审计记录至少包含：

```text
host uid/gid/pid
external subject
space
agent
operation
object class
provider
model
tool
mcp server
decision
latency
token usage
cost
error
fingerprint
```

密钥不写日志。敏感 prompt 是否记录由 policy 决定。

## 22. 训练数据导出

```text
export/
  conversations.jsonl
  sft.jsonl
  preference.jsonl
  tool_calls.jsonl
  agent_traces.jsonl
  refresh
  filter/
```

```text
export/filter/
  provider
  model
  agent
  subject
  space
  from
  to
  exclude_failed
```

导出来源：

```text
home/*/thread/*/messages.jsonl
home/*/thread/*/tool-loop/steps.jsonl
home/*/api/*/inbox
home/*/api/*/outbox
ext/*/.../thread/*/inbox/*.req.json
tool/*/invoke/inbox/*.req.json
agent/*/inbox/*.req.json
home/*/audit/events.jsonl
home/*/memory/episodic
home/*/feedback/preference/inbox/*.req.json
```

要求：

- 可追溯到 thread/request/fingerprint。
- 支持脱敏。
- 支持按 provider/model/agent/subject/space/time 过滤。
- 支持排除失败样本。
- 支持导出 tool call 和 tool result。
- 支持 preference pair。
- 支持去重。

## 23. 安全模型

CortexFS 使用 SELinux 风格安全上下文。

身份层：

```text
HostActor  本机 uid/gid/pid
Subject    被代表用户，例如 chat:user:123456
Object     文件系统对象或 AI 资源
```

Context：

```text
identity:role:type:level
```

示例：

```text
local:uid1000:user_r:chat_client_t:s0
local:uid1000:adapter_r:chat_adapter_t:s0
chat:user123456:member_r:chat_member_t:s0:c_chat,c_room888888
chat:room888888:object_r:room_thread_t:s0:c_chat,c_room888888
```

Object classes：

```text
space
thread
message
request
response
provider
model
secret_ref
cache_entry
audit_log
control
route
policy
socket
mcp_server
mcp_tool
skill
agent
cluster
memory
vector_index
database
```

权限动词示例：

```text
read write append submit invoke connect stream cancel
use configure healthcheck rotate inspect export relabel
claim schedule delegate handoff remember retrieve
```

访问流程：

```text
FUSE request -> HostActor
optional adapter verified Subject
resolve path -> Object context
Unix mode check
Cortex policy check
allow or EACCES
audit
```

对象 context 可通过 xattr 暴露：

```bash
getfattr -n user.cortex.context <path>
```

## 24. 外部平台

外部群聊和机器人平台不是 Linux 用户。

```text
ext/chat/room/888888/
  subject/
  thread/
  agent/
  policy/
```

Subject：

```text
subject/123456/
  display_name
  role
  permissions
  quota/
```

消息可带 subject：

```jsonl
{"role":"user","content":"帮我总结","subject":"chat:user:123456","display_name":"Alice"}
```

只有可信 adapter domain 可以代表 external subject 写入。

## 25. 控制节点

全局：

```text
control/
  flush
  gc
  drain
```

Space：

```text
home/1000/control/
  gc
```

Agent：

```text
agent/helper/control/
  start
  stop
  restart
  pause
```

Cluster：

```text
cluster/main/control/
  rebalance
  drain
  pause
```

控制文件 write-only：

```bash
echo 1 > control/drain
```

## 26. 外部编排软件集成

CortexFS 可以作为 workflow engine、agent runtime、数字人系统和批处理脚本的执行面，但不得为任何上层项目增加专属路径。上层软件只能依赖稳定文件 ABI 和发现文件。

最小集成契约：

```text
模型发现      读 home/<uid>/model/{count,list} 和 format/<format>/model/list
路由观察      读 home/<uid>/route/<format>/{provider,model,reason}
API 提交      rename 到 home/<uid>/api/<format>/inbox/<id>.req.json
Thread 批处理 rename 到 home/<uid>/thread/<id>/inbox/<id>.req.json
Thread 实时   连接 home/<uid>/thread/<id>/io.sock
Tool 调用     rename 到 tool/<tool-id>/invoke/inbox/<id>.req.json
MCP 调用      rename 到 mcp/tool/<server>.<tool>/invoke/inbox/<id>.req.json
记忆写入      rename 到 home/<uid>/memory/<layer>/inbox/<id>.req.json
审计读取      读 audit/events.jsonl
训练导出      读 home/<uid>/export/*.jsonl
实时 fast path 连接对应 io.sock 或 api.sock
```

无论请求来自文件、HTTP 还是 Unix socket，都必须产生同一组派生事实：

```text
request id
fingerprint
route metadata
policy decision
audit event
export row
thread/tool-loop update when bound
```

外部软件不得假设 provider id、model id、agent id、uid、平台 subject 或 demo thread 名称。`home/1000`、`agent/helper`、`ext/chat/room/888888` 这类路径只是示例；正式 ABI 模式是 `home/<uid>`、`agent/<agent-id>`、`ext/<platform>/...`。

外部编排器如果需要表达自己的 run/step，应把它写进请求 JSON、thread metadata 或 audit subject/agent context；不要要求 CortexFS 增加 `<project>/`、`workflow/`、`pipeline/` 这类上层项目目录。CortexFS 只提供 provider、format、policy、tool、memory、audit 和 export 的通用执行面。

同理，不要要求 CortexFS 增加 `chan/`、`job/` 或 `hook/`。中转站和账号实例是 `provider/`，路由选择是 `home/<uid>/route/`，外部触发器直接写对应 inbox。

socket 只能降低延迟，不能改变语义。socket 接入必须校验 peer credential，进入同一 policy/route/store/audit/export 管线。文件树是可审计 source of truth。交互式 agent 软件不应把每轮对话降级成 `thread/inbox` 文件提交；`thread/inbox` 是批处理和非交互兼容入口。

## 27. 实现架构

Rust workspace：

```text
crates/
  cortex-core
  cortex-store
  cortex-provider
  cortex-mcp
  cortex-skill
  cortex-tool
  cortex-agent
  cortex-memory
  cortexd
  cortexfs
  cortex-cli
```

职责：

```text
cortex-core       类型、ABI、security context、fingerprint
cortex-store      sqlite/postgres/CAS/cache/audit
cortex-provider  AI provider adapters
cortex-mcp        MCP client/server/session 管理
cortex-skill     skill registry/index/load
cortex-tool      tool registry/invocation/tool loop
cortex-agent     agent runtime/collab/cluster primitives
cortex-memory     memory/vector/db integration
cortexd           daemon 执行平面
cortexfs          FUSE 投影
cortex-cli        init/start/stop/restart/mount/daemon/status
```

## 28. MVP

第一版必须小，但目录设计不堵死后续。

MVP 实现：

```text
单用户挂载
一个 local user space
openai.chat inbox/outbox
至少两个支持 openai.chat 的 provider url
provider model/count
space model/count
thread messages.jsonl
thread io.sock
fingerprint
audit/events.jsonl
简单 route/default_provider
简单 policy/allowed_provider
secret status
export/conversations.jsonl
mcp server/tool 只读 registry 骨架
skill installed/ 只读 registry 骨架
agent/<id>/profile/runtime 骨架
```

暂缓：

```text
完整 agent 集群调度
完整 SELinux policy language
完整 MCP server 生命周期
完整 skill 执行
完整 Postgres backend
完整向量数据库
完整数字人
完整训练数据清洗
```

## 28. 验证要求

每个暴露文件都要测试：

```text
getattr
read
write if writable
rename submit
invalid input
permission denied
unsupported operation
crash/restart recovery where applicable
audit side effect
policy side effect
```

质量门禁：

```text
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
pre-commit run --all-files
```
