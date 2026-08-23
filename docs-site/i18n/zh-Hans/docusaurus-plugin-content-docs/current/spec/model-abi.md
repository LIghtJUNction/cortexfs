# 模型 ABI

当前仅有一个模型 ABI：

```text
/ctx/model/<provider>/<model>       one-shot 推理可执行对象
/ctx/model/<provider>/<model>.sock  可选的 CortexFS 会话套接字
/ctx/model/<provider>/<model>.d/    控制文件
/ctx/model/main                     默认模型别名
/ctx/model/{helper,fast,reason,code,vision}
                                    规范兼容能力别名
```

`<provider>/<model>` 由两个路径组件组成。对于原生模型提供商，`<provider>` 是原始提供商标识，例如：

```text
/ctx/model/openai/gpt-5.6
/ctx/model/anthropic/claude-sonnet-5
/ctx/model/google/gemini-3.6-flash
```

对于未声明原始提供商映射的自定义域名 base URL，`<provider>` 使用标准化主机名，例如：

`https://models.example.test:9000/`

投影为：

```text
/ctx/model/models.example.test/compatible-model
```

地址类端点如 `127.0.0.1`、`::1`、`localhost` 必须在宿主侧提供商配置中显式设置 `name`。未设置时配置无效，因为 `/ctx/model/<provider>` 必须是稳定对象名，而不是传输地址。示例：

```json
{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "default_model": "custom-model",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
```

该配置会项目化为 `/ctx/model/local/custom-model`。

自定义 base URL 属于提供商适配器配置，不是 root ABI 命名空间。它可在
`model/<provider>/<model>.d/default` 中供检查显示，但 secrets 绝不能出现在模型元数据或 `.d/` 文件中。

底层 AI API 格式不进入 ABI。OpenAI Responses、Anthropic Messages、Gemini
GenerateContent、OpenAI 兼容 chat、local runtimes 与聚合器专有请求格式都属于协议适配器
细节。底层有状态/无状态行为不进入 ABI，CortexFS 协议适配层负责把提供商连接、API
兼容性和流式行为适配为稳定的 CortexFS 请求与 JSONL 事件流。

示例：

```text
/ctx/model/
  main -> /ctx/model/openai/gpt-5.6
  helper -> /ctx/model/openai/gpt-5.6-sol
  fast -> /ctx/model/openai/gpt-5.6
  reason -> /ctx/model/openai/gpt-5.6
  code -> /ctx/model/openai/gpt-5.6
  vision -> /ctx/model/openai/gpt-5.6
  debug/
    echo
    echo.d/
      id
      driver
      cap
      effort
      default
      fallback
      limit
      session
      status
      log
  openai/
    gpt-5.6
    gpt-5.6.d/
      id
      driver
      cap
      effort
      default
      fallback
      session
      status
      log
```

`main`、`helper`、`fast`、`reason`、`code`、`vision` 是完整的稳定别名集。`helper` 保留为兼容别名。bootstrap 可能选择一个与能力别名匹配、提供商无关的模型投影；无法识别时该别名指向选择后的 `main`。已有的有效用户管理别名软链接保留。

控制文件：

```text
id       提供商原生或运行时内部模型 id
driver   路由表；见下文
cap      能力列表，每行一个
effort   与提供商无关的推理强度：auto, none, low, medium, high, xhigh, 或 max
default  默认参数，KEY=VALUE，每行一个
fallback 有序的回退模型链，每行一个 provider/model
limit    以 token 计的硬上下文上限；或 unknown
session  none 或 socket
status   动态状态
log      简短调用日志或日志位置指针
```

## 硬上下文限制

每个模型控制目录包含只读 `limit` 文件：

```text
/ctx/model/<provider>/<model>.d/limit
```

该文件恰好一行 LF 结尾文本。取值为 `unknown` 或正整数 `u32` token 数。
数字文本不允许符号、空白和前导零。零、溢出、额外行以及非规范十进制文本均无效。
该值是该提供商/模型的硬上下文上限，不是输出 token 设置，不能当作输出上限使用。

示例：

```text
272000
unknown
```

`unknown` 表示 CortexFS 没有可信最大值，不得将其渲染为 0，也不得用猜测值替换。可执行模型元数据字段 `context_length` 与 `limit` 的规范值一致。

`limit` 是可检查投影，绝不属于 Agent 可写控制。FUSE 对其请求变更尝试必须失败并返回 `EROFS`（含 uid 0）。更新限制仅在主机配置变更或现有同步挂载目录刷新时发生；不存在 watcher、poller、hot-reload 路径。

解析优先级：

```text
1. 所选主机提供商配置中的 model_limits
2. 有效的 CortexFS-owned models.dev 缓存项
3. unknown
```

提供商配置可在不修改兼容的 `models` 列表前提下，为本地模型声明显式限制：

```json
{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "models": ["custom-model"],
  "model_limits": {
    "custom-model": 32768
  }
}
```

每个 `model_limits` 键必须是 `default_model` 或 `models` 中声明的模型，
每个值必须在 `1..=4294967295`。本地限制声明无效则配置无效，不能静默忽略。
本地条目覆盖同一投影模型的目录数据。

提供商配置还可覆盖已声明模型的稳定语义能力：

```json
{
  "name": "local",
  "base_url": "http://127.0.0.1:8317/v1",
  "models": ["text-model", "vision-model"],
  "model_capabilities": {
    "text-model": ["chat", "stream"],
    "vision-model": ["chat", "stream", "vision"]
  }
}
```

每个键必须是 `default_model` 或 `models` 中的模型。值必须是下方列表中的稳定能力词且唯一。提供商私有、未知或重复能力会使配置无效。显式空列表是合法的，会投影空 `cap` 文件。未显式覆盖的模型使用适配器派生能力。

CortexFS 通过外部 `models-dev` 库获取目录限制。Catalog 的提供商与模型映射键必须与投影身份
`<provider>/<model>` 精确匹配；传输主机和聚合器名不能作为原始 provider。
只有稳定的 CortexFS 提供商/模型名称和正整数 limit 会进入缓存。

主机缓存形状：

```json
{
  "schema": "cortexfs.model-limits/v1",
  "models": {
    "openai/gpt-5.6": 272000
  }
}
```

仅在一次完整在线响应解析通过后，缓存才会原子替换。超时、网络错误、无效/过大响应、空验证结果或持久化写入失败都会保留旧缓存。缺失、格式错误、过大、schema 不匹配或不安全的缓存不提供上限。缓存内容不包含 provider secrets，是后端状态，不是新的 `/ctx` 命名空间。

`fallback` 是模型回退链，不是传输路线。它位于 `model/<provider>/<model>.d/fallback`；每一行是一个稳定 `provider/model` 引用（不含注释），例如：

```text
openai/gpt-5.6
models.example.test/compatible-model
```

当选中模型不可用或在成功生成前失败时，运行时按顺序尝试回退模型。每个候选仍走正常的 provider registry、secret lookup 与 `/ctx/model/route` 出口规则。

`driver` 可能是旧式单驱动名：

```text
debug
```

或路由表：

```text
default=openai-chat
exec=openai-chat
socket=openai-chat
agent=openai-responses,openai-chat
```

路由键：

```text
default  回退路由
exec     直接 one-shot 模型执行
socket   直接模型套接字调用
agent    agent 拥有的模型调用
```

每个值是逗号分隔的优先级列表。运行时先按用途路由再回退到 `default`。这种机制允许直接模型使用经典聊天驱动，而 agent 调用偏向更丰富的 Responses 风格并可回退到 chat。driver 名是适配器名，不是稳定的模型名。

适配器名及默认选中它的 provider `formats` 条目：

```text
openai-chat         openai.chat         POST <base>/chat/completions
openai-responses    openai.responses    POST <base>/responses
anthropic-messages  anthropic.messages  POST <base>/messages
google-generative   google.generative   POST <base>/models/<model>:generateContent
```

OpenAI 与 Anthropic 适配器会把 provider base URL 归一化为 `/v1` 后缀。`google-generative` 则原样使用 base URL，因为 Gemini 把 API 版本写在其中（`.../v1beta`），并把模型绑定到请求路径；它用 `x-goog-api-key` 携带 API key，用 `Authorization: Bearer` 携带 OAuth access token。不在此集合内的适配器名会在发出任何请求之前返回稳定错误。

机密信息绝不存于模型文件或 `.d/` 控制文件。Provider 凭据按以下优先级读取：

```text
root-owned CortexFS 系统密钥存储
unconfigured
```

API key 从
`/var/lib/cortexfs/secrets/provider/<provider>/<slot>` 读取。Provider JSON 不得声明
API key 的环境变量名，密钥也不能放入进程环境。若系统密钥缺失，模型视为未配置并应返回稳定错误，除非端点支持免认证请求。

OAuth 提供商遵循同样规则：access token 是 bearer credential，并保持在 provider runtime 状态，不进入模型 ABI。Provider JSON 可以声明 OAuth Authorization Code + PKCE：

```json
{
  "base_url": "https://api.example.com/v1",
  "oauth": {
    "client_id": "cortexfs-local",
    "auth_url": "https://auth.example.com/oauth/authorize",
    "token_url": "https://auth.example.com/oauth/token",
    "redirect_uri": "http://127.0.0.1:8765/callback",
    "scopes": ["model.read", "offline_access"]
  }
}
```

OAuth token 环境变量名由提供商身份派生，例如 `CTX_EXAMPLE_OAUTH_ACCESS_TOKEN`、
`CTX_EXAMPLE_OAUTH_REFRESH_TOKEN`；用户不应在 provider JSON 中配置这些名称。
若派生出的 access token 变量不存在或为空，运行时会查找
`service=cortexfs:<provider> account=oauth:access`。
refresh token（若由 provider adapter 或 CLI wrapper 使用）默认查找
`account=oauth:refresh`。PKCE 使用 `S256`，验证器与回调 state 是短命本地流程状态，不得写入 `/ctx/model`。`ctx provider oauth login PROVIDER` 是宿主侧助手，用于完成该 PKCE 流程并将 token 写入系统 keychain。

## Provider 预设

Provider 预设是宿主侧 JSON 模板文件，安装于 `/etc/cortexfs/providers.d/`，不会建立 `/ctx/provider` 命名空间：

```text
ctx provider preset list
ctx provider preset show openai|codex|anthropic|google
ctx provider preset install openai|codex|anthropic|google
```

标准 provider 名称：

```text
openai     Agent 调用使用 `/v1/responses`，备用 `/v1/chat/completions`；`codex` 为别名
anthropic  Claude Messages API
google     通过 Google 的 OpenAI 兼容端点访问 Gemini；`gemini` 为别名
```

`codex` 别名安装 OpenAI 预设并在规范提供商路径下投影 Codex 推荐的模型，例如 `/ctx/model/openai/gpt-5.6`。它不创建 `/ctx/model/codex` 或第二个提供商命名空间。

Google 预设使用 Gemini 的 OpenAI 兼容端点。Anthropic 预设使用 `anthropic.messages`，因此运行器发送 `POST /v1/messages` 并携带必需的 Anthropic 版本 header。

## 一次性执行

`/ctx/model/<provider>/<model>` 是只读可执行对象。读取该路径返回该模型的 CortexFS 元数据文本。执行会进行一次性推理：通过 CortexFS/Rust 运行时代码或 provider adapter；模型对象不得是 shell 脚本实现。

首批元数据字段对应通用模型列表字段：

```text
id
name
description
type
created_at
owned_by
context_length
```

提供商适配器可从 `ModelListingClient::list_models()` / `ModelList` 填充这些字段。内置 `debug/*` 模型是本地调试元数据，不代表 provider 默认。

```bash
/ctx/model/debug/echo "hello"
echo "hello" | /ctx/model/openai/gpt-5.6
echo '{"messages":[{"role":"user","content":"hello"}]}' | /ctx/model/openai/gpt-5.6
```

语义：

```text
one invocation
no durable session mutation
stdout 是规范 JSONL 事件流
exit code 是进程级摘要
文件内容可检查元数据，不是 provider 代码或 secrets
```

即使底层 provider 有本地状态，`/ctx/model/<provider>/<model>` 仍表现为无状态的一次性调用。

## 全局模型路由

模型代理不是 agent，也不存于 provider JSON。全局出站路由表只有一份：

```text
/ctx/model/route
```

该文件是普通 CortexFS 状态。仅在发起模型请求时读取；若文件缺失，默认 `fallback: direct`。

规则从上到下评估。规则选择一个 group；group 同时选择传输方式和可选凭据槽。凭据不会写入路由文件或 provider JSON。`key(NAME)` 会在运行时从系统密钥存储读取
`/var/lib/cortexfs/secrets/provider/<provider>/NAME`。API key 不放入进程环境。缺少 `key(...)` 时默认凭据槽是 `default`。

```text
group(proxy) -> http(http://127.0.0.1:8080/v1), key(office)
group(local-socket) -> unix(/run/user/1000/cortexfs/proxy/openai.sock), key(local)

dip(198.51.100.45) -> direct
# dip(203.0.113.43) -> JP
domain(bestproxy.com) -> proxy
pname(NetworkManager, systemd-resolved, dnsmasq) -> must_direct
dip(geoip:private) -> direct
dip(geoip:cn) -> direct
domain(geosite:cn) -> direct
model(embedding-*) -> local-socket
fallback: proxy
```

内置组名：

```text
direct       使用 provider base_url 与默认凭据槽
must_direct  与 direct 相同传输，仅用于策略可读性
```

自定义组：

```text
group(NAME) -> direct[, key(SLOT)]
group(NAME) -> http(BASE_URL)[, key(SLOT)]
group(NAME) -> unix(SOCKET_PATH[, BASE_URL])[, key(SLOT)]
```

匹配器当前包括 `domain(...)`、`dip(...)`、`pname(...)`、`provider(...)`、`model(...)`。
`model(...)` 与 `provider(...)` 接受完整名与后缀 `*` 前缀。

## 模型套接字

`/ctx/model/<provider>/<model>.sock` 是唯一的多轮模型入口，采用与 [object-abi.md](object-abi.md) 相同的 JSONL 套接字协议：

```jsonl
{"op":"send","id":"msg-1","session":"default","input":"hello"}
{"op":"resume","session":"default","after":"event-123"}
{"op":"cancel","id":"run-1"}
{"op":"ping"}
```

模型套接字会话语义使用 CortexFS 语义，而不是 provider 本地会话；provider 线程、response id、上下文缓存和模拟消息日志在协议层被隐藏。

`model/<provider>/<model>.d/session` 仅有两个稳定值：

```text
none    无 /ctx/model/<provider>/<model>.sock
socket  /ctx/model/<provider>/<model>.sock 存在，并支持 CortexFS 会话
```

该值仅描述 CortexFS 状态，不会描述 provider 本地状态。

## 能力

使用稳定语义能力词：

```text
chat
stream
session
vision
audio_input
audio_output
json_schema
tool_call_syntax
reasoning
embedding
rerank
```

提供商私有或 API 格式私有的能力词在稳定 ABI 中禁止：

```text
openai_responses
anthropic_messages
gemini_generate_content
native_thread
native_stateful
native_stateless
```

`tool_call_syntax` 仅表示模型事件流可能包含 tool-call 形态的事件，不意味着模型可执行工具。它不授予任何工具权限。

## 规范事件流

模型与 agent 流共享事件类型：

```text
start
delta
message
reasoning_delta
reasoning_message
tool_call
usage
error
done
```

示例：

```jsonl
{"type":"start","run":"r1","model":"debug/echo"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"message","run":"r1","role":"assistant","content":[{"type":"text","text":"hello"}]}
{"type":"usage","run":"r1","input_tokens":10,"output_tokens":1}
{"type":"done","run":"r1","status":"ok"}
```

`usage` 需要 `input_tokens` 和 `output_tokens`。由 provider 报告时，可选的 `cached_tokens` 与 `cache_write_tokens` 记录缓存读写，不改变总量统计。

错误示例：

```jsonl
{"type":"error","run":"r1","code":"EACCES","message":"permission denied"}
{"type":"done","run":"r1","status":"error"}
```

`code` 使用稳定 errno 名称，客户端不得解析 `message`。

## 原生诊断

`model/<provider>/<model>.d/native` 仅用于诊断：

```text
native 是诊断用途
native 不是稳定 ABI
strict clients 不应依赖它
```

## 工具边界

模型执行不等于工具执行。

硬规则：

```text
model 可产生 tool_call 事件
model 不得执行工具
agent 决定是否执行工具
agent policy 决定是否允许执行
```

模型进程不得接收项目挂载、工具凭据或写权限（除运行时可写缓存）。Provider 工具调用不得成为 bypass agent policy 的后门。
