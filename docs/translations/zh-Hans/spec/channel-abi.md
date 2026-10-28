# 通道 ABI

本文档定义公共 `cortexfs-channels` crate 边界与可选 CortexFS 通道 host。它不会新增
`/ctx/channel` 树或第二套持久提交机制。通道交付通过现有 `agent/<name>.sock` 的
JSONL 会话 ABI 进入系统。

## 公共 crate

发布包为 [cortexfs-channels on crates.io](https://crates.io/crates/cortexfs-channels)。

`cortexfs-channels` 与运行时无关。它不依赖 CortexFS、FUSE、模型 provider、HTTP 客户端
或特定异步执行器。它导出：

- `ChannelAdapter`：对象安全的 connect/start、receive/listen、send、reconnect、capability
  与 health 方法；
- `ChannelRegistry`：命名适配器注册与派发；
- `InboundMessage` 与 `OutboundMessage`：统一的会话、线程、回复 id、参与者、文本、
  附件与元数据模型；
- `ChannelSessionRoute`：确定性 conversation-to-session 映射；
- `ChannelEnvelope`：带版本的 JSON 边界，ABI 值为 `cortexfs.channel/v1`；
- `ChannelFrame`：双向 JSONL socket 边界，ABI 值为
  `cortexfs.channel.socket/v1`，携带相关 id、生命周期、健康、投递、回执与实时效果；
- `platform::{telegram, discord, slack, feishu, dingtalk, matrix, whatsapp, gmail, email, signal, irc, mattermost, qq}`：
  无状态 payload codec。

适配器负责认证、限速、重连策略与平台传输，公共层仅负责语义约定。host 可在不修改
任何 agent 或文件系统 ABI 的前提下实现新适配器。打包的 Discord host 从仅所有者可
读 TOML 文件读取凭据与路由值；它不会将通道状态写入 `/ctx`。

## 消息与会话语义

默认 route 将 `(channel, conversation, thread)` 视为一条多轮上下文。服务多个外部用户
的 host 可以启用同一泛 route 的身份隔离模式；此时发送方身份会纳入稳定 key，但不会引入
Telegram/Discord 专有字段。CortexFS 内置 host 为私聊会话启用此模式。它将选定 key
映射为 `im-<prefix>-<stable-hash>` 并通过现有请求提交文本：

```json
{"op":"send","id":"im-<prefix>-<message-hash>","session":"im-<prefix>-<conversation-hash>","scope":"private","input":"hello"}
```

现有 agent socket 文件仍是幂等、持久化 `messages.jsonl`/`events.jsonl`、工具授权、模型
执行与助手 event framing 的事实真相。相同平台消息的重复投递会重放已有会话结果，而非
再次执行 agent。

该 bridge 可增量消费同一流。内置交互 host 使用 `start`、`delta`、`message`、`error` 与
`done` frame 提供传输级别的确认、输入中状态提示、有界占位编辑与最终消息。
这些进度效果不写入会话历史；socket 保持真相来源。除非 host 明确发送面向用户失败提示，
否则错误与工具事件不作为 assistant 文本暴露。

socket driver 边界与前端/运行时交互 ABI 是刻意分离的。driver 向通道运行时发送
`Inbound` frame；运行时返回 `Deliver` 或 `Effect` frame。`Deliver`、`Effect`、`Event`、
`Health` 与 `Receipt` 是独立流 frame，不是锁步 request/response 一一对应：连接建立后任
一方都可发送自身 frame。这使得运行时主动投递和 driver 心跳不会与最近一次入站消息耦合。
`Typing` 与 `Preview` 效果是实时、有界进度信号：它们不是持久消息，driver 在平台无对应
能力时可忽略。runtime 在到达时写入这些效果并只保留有界预览文本，避免长模型流在内存
中累积全部 delta。`Start` 与 `Stop` 描述生命周期，不将 adapter 与某个 Agent 或文件系统路径
绑定。`request_id`/`event_id` 用于关联重试和回执，不替代持久会话幂等规则。

运行时命令仍复用 agent socket 的同一交互 ABI。terminal host 可用 `CommandResult` 回应
`approval_request`，runtime 会校验原始 request id、run 与 tool-call id。当前单次 Web
POST 与 channel bridge 故意在交互命令上返回有界拒绝，因为二者都没有第二条客户端到运行时
流；它们不会自动批准工具调用，也不会让运行时等待不可用回复。

内置 `cortexfs-channel web` host 在同一可配置路径上，通过 `POST /v1/interaction` 与 WebSocket
双通道暴露同一 interaction frame。POST 接受一个 `cortexfs.interaction/v1` 请求，并将运行时
事件流作为换行分隔 JSON 返回。WebSocket 客户端发送初始请求为一条交互 frame，接收相同事件
frame，并可在验证请求、会话与命令 id 后在该连接上回应运行时命令。其 run 活动期间，
客户端还可发送 `input`、`resume`、`status` 或 `cancel`；host 将每个请求通过独立已有
agent-socket stream 提交，并将事件回并到同一 WebSocket。故连接是全双工，而不改变 agent socket
ABI 或引入 web 专有命令类型。默认绑定地址为 loopback；绑定非 loopback 时需要
`CORTEXFS_WEB_TOKEN`，以 HTTP Bearer token 校验。

## 内置 host

`cortexfs-channel` 为显式前台进程。它不是 `ctx` 子命令，不创建新 root namespace，也不
监听文件。`cortexfs-channel web` 用于浏览器/API 前端，`webhook` 用于平台原生回调；二者都进入
同一 agent socket 交互 ABI。Discord 使用 `/etc/cortexfs/channels/discord.toml` 配置并通过
Gateway 运行。Telegram 使用长轮询，DingTalk 使用 Stream Mode，Matrix 使用 Client-Server
`/sync` 循环；webhook 仍用于其他无状态 host。

内置配置合同使用公共客户端端点 `/ctx/agent/<agent>.sock`，由
`cortexfs-paths::agent_client_socket` 派生。私有 `/run/cortexfs/agent/<agent>.sock` 路径为
systemd 实现细节，会被 channel 配置加载器拒绝。host 必须在宣告可用前验证所选 storage
generation 与活动 agent socket。

```bash
# Discord Gateway
sudo systemctl enable --now cortexfs-channel@discord.service

# DingTalk Stream Mode
export CORTEXFS_DINGTALK_CLIENT_ID='read-from-a-secret-store'
export CORTEXFS_DINGTALK_CLIENT_SECRET='read-from-a-secret-store'
cortexfs-channel dingtalk

# Matrix Client-Server sync
export CORTEXFS_MATRIX_HOMESERVER='https://matrix.example.org'
export CORTEXFS_MATRIX_ACCESS_TOKEN='read-from-a-secret-store'
cortexfs-channel matrix

# Slack Events API 或 Feishu/Lark webhook
export CORTEXFS_CHANNEL_PLATFORM=slack
export CORTEXFS_CHANNEL_BIND=127.0.0.1:8765
export CORTEXFS_CHANNEL_OUTBOUND_URL=https://slack.com/api/chat.postMessage
export CORTEXFS_CHANNEL_TOKEN='read-from-a-secret-store'
cortexfs-channel webhook
```

webhook 模式下，将平台回调 URL 配置为 `http://<host>:8765/webhook`（或已配置的
bind/path），并在监听器前放置签名校验反向代理。内建监听器仅校验 framing 与 body 大小，
平台签名校验是部署策略，不能静默绕过。

codec 目前覆盖 Telegram 文本更新、Discord message/webhook payload、Slack message events 与
URL 验证、Feishu/Lark 文本事件、DingTalk Stream callback frame、Matrix `m.room.message`
事件、WhatsApp Business Cloud text webhooks/Graph API 发送、Gmail Pub/Sub 与 message 资源、
RFC 5322 email、Signal CLI envelope、IRC `PRIVMSG`、Mattermost `posted` WebSocket 事件/REST
post 以及 QQ Bot Gateway 的 guild/group/C2C 事件与 REST post。
WhatsApp 通过通用 webhook host 配置 `CORTEXFS_CHANNEL_PLATFORM=whatsapp` 选择；outbound URL
必须包含 Graph API 的 phone-number endpoint，bearer token 由 host 传递。
Discord 与 Telegram 前台 host 支持尽力而为的 reactions、typing indicators、占位消息与
合并编辑；DingTalk 回复使用 per-message 会话 webhook。Matrix 回复使用
`m.in_reply_to` 或 thread 关系。webhook host 在其平台特定编辑约定未配置前为最终消息一次性发送。
媒体与签名/加密能力仍是显式能力待实现项，不应被当作普通文本静默处理。
