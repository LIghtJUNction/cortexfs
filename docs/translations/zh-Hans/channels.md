---
sidebar_position: 5
---

# 多平台 IM 通道

CortexFS 将 IM 视为现有 agent 会话之上的传输层，而不是新增文件系统命名空间。
可复用层是可发布的 `cortexfs-channels` crate；CortexFS 仅提供一个小型 bridge 与
前台 host。该 crate 在 [crates.io](https://crates.io/crates/cortexfs-channels)
发布，其 API 可在 [docs.rs](https://docs.rs/cortexfs-channels) 浏览。

## 安装与启动

先用宿主目录命令查看家族、传输和模板，无需启动传输：

```bash
cortexfs-channel list
cortexfs-channel show telegram
cortexfs-channel preset telegram
```

`list` 输出 `家族<TAB>传输<TAB>native|driver<TAB>命令`。`show` 打印所需密钥与
systemd unit。`preset` 把仅所有者可读的环境模板打到 stdout，再重定向到
`/etc/cortexfs/channels/<family>.env` 填入密钥。这是宿主助手，不是 `ctx`
子命令，也不是第二套提交入口。

Telegram、Discord、Slack 会话还接受 `/help`、`/models`、`/model`、
`/model PROVIDER/MODEL` 与 `/new`；由 channel bridge 直接回答，不引入平台
专有 ABI。

安装正常的 CortexFS 包后，启动一个 agent 并将 Discord 适配器配置写入一个仅
所有者可读写的文件：

```bash
ctx agent start executor --session default
sudo install -d -m 0700 /etc/cortexfs/channels
sudoedit /etc/cortexfs/channels/discord.toml
sudo chmod 600 /etc/cortexfs/channels/discord.toml
```

文件包含应用 ID、bot token、agent socket 路径与 agent 名称。token 仅从该文件读取，
并在诊断输出中脱敏：

```toml
application_id = "DISCORD_APPLICATION_ID"
bot_token = "DISCORD_BOT_TOKEN"
agent_socket = "/ctx/agent/executor.sock"
agent = "executor"
session_prefix = "discord"
```

在 Discord 开发者门户开启 `MESSAGE_CONTENT` 特权权限后，启动低内存的同步
Gateway 适配器：

```bash
sudo systemctl enable --now cortexfs-channel@discord.service
sudo journalctl -u cortexfs-channel@discord.service -f
```

该适配器维护一条有界 WebSocket 连接，并使用已有的持久会话 ABI 公共端点
`/ctx/agent/<agent>.sock`。它会在连接前校验该端点为活跃的 Unix socket。
它不会新增 `/ctx/channel` 命名空间、watcher 或轮询 worker。

更改 provider 或 model 后，在启动通道前刷新持久化 backing generation：

```bash
sudo ctx storage update --prune /var/lib/cortexfs/storage
sudo systemctl restart cortexfs.service
sudo systemctl restart cortexfs-agent@executor.socket
sudo systemctl restart cortexfs-channel@discord.service
sudo ctx doctor
```

需要刷新的原因是 `/ctx` 只是投影，而 agent runtime 直接读取所选 backing generation。
仅凭投影中的 model 条目不足以证明运行时能够解析该 model。

Telegram 使用长轮询：

```bash
export CORTEXFS_TELEGRAM_TOKEN='...'
cortexfs-channel telegram
```

DingTalk 使用官方 Stream Mode 网关。将两类凭据放在服务管理器的 secret
环境中，并按 agent 启动一个前台 host：

```bash
export CORTEXFS_DINGTALK_CLIENT_ID='...'
export CORTEXFS_DINGTALK_CLIENT_SECRET='...'
cortexfs-channel dingtalk
```

面向打包部署时，将完整运行时配置放入仅所有者可读文件，并让 systemd 负责重连：

```bash
sudo install -m 600 /dev/null /etc/cortexfs/channels/dingtalk.env
sudoedit /etc/cortexfs/channels/dingtalk.env
sudo systemctl enable --now cortexfs-channel-dingtalk.service
```

该文件至少包含 `CORTEXFS_AGENT`、规范化的
`CORTEXFS_AGENT_SOCKET`、`CORTEXFS_DINGTALK_CLIENT_ID`、`CORTEXFS_DINGTALK_CLIENT_SECRET`。
可选项包括 `CORTEXFS_CHANNEL_SESSION_PREFIX`、`CORTEXFS_AGENT_CWD` 和
`CORTEXFS_DINGTALK_GATEWAY_URL`。

host 会确认 gateway frame、在 WebSocket 断开后重连，并通过通用身份隔离 route
映射私聊与群聊。它通过 DingTalk 的每条消息会话 webhook 发送 Markdown 回复。
会话 webhook 是短暂通道状态，不会写入 `messages.jsonl`、`events.jsonl` 或 `/ctx`。

Matrix 使用带 bearer token 的 Client-Server API。适配器先执行 `whoami`，再维护一条
有界的 `/sync` cursor，并通过 Matrix 线程/回复关系发送文本回复：

```bash
export CORTEXFS_MATRIX_HOMESERVER='https://matrix.example.org'
export CORTEXFS_MATRIX_ACCESS_TOKEN='...'
# 可选，逗号分隔的 room IDs：
export CORTEXFS_MATRIX_ROOMS='!room:example.org'
cortexfs-channel matrix
```

打包服务部署时，将这些变量写入 `/etc/cortexfs/channels/matrix.env`（权限 0600），
然后执行 `sudo systemctl enable --now cortexfs-channel-matrix.service`。access token
不会存放在 `/ctx` 或会话文件中。

Mattermost 使用其原生 WebSocket 事件流和 REST API 发表消息。为打包 host 设置仅
所有者可读的环境文件：

```bash
sudo install -m 600 /dev/null /etc/cortexfs/channels/mattermost.env
sudoedit /etc/cortexfs/channels/mattermost.env
sudo systemctl enable --now cortexfs-channel-mattermost.service
```

该文件要求包含 `CORTEXFS_AGENT`、`CORTEXFS_AGENT_SOCKET`、`CORTEXFS_MATTERMOST_URL`
与 `CORTEXFS_MATTERMOST_TOKEN`。可选设置 `CORTEXFS_MATTERMOST_CHANNELS`（逗号分隔
allowlist）和 `CORTEXFS_MATTERMOST_RECONNECT_SECONDS`。Mattermost 的线程回复会映射到
通用的 `thread` 字段；在配置了有界文件传输能力之前，attachments 会被拒绝。

QQ 使用 Bot API Gateway 接收 guild、group 与 C2C 事件，并使用对应 REST 接口回执。
为打包 host 配置仅所有者可读环境文件：

```bash
sudo install -m 600 /dev/null /etc/cortexfs/channels/qq.env
sudoedit /etc/cortexfs/channels/qq.env
sudo systemctl enable --now cortexfs-channel-qq.service
```

文件要求 `CORTEXFS_AGENT`、`CORTEXFS_AGENT_SOCKET`、`CORTEXFS_QQ_APP_ID` 与
`CORTEXFS_QQ_TOKEN`。可选项为 `CORTEXFS_QQ_INTENTS`、`CORTEXFS_QQ_API_BASE`、
`CORTEXFS_QQ_GATEWAY_URL` 与 `CORTEXFS_QQ_RECONNECT_SECONDS`。Guild/group/C2C 目标
保留在通用 message 元数据中，因此核心 message ABI 不会新增 QQ 特定字段；在配置了
有界上传能力前，媒体附件会被拒绝。

Gmail Push 使用一个小型 Pub/Sub 回调监听器与 Gmail history API。listener 只接收一条
history cursor，然后用 bearer token 拉取消息内容并把每个发送者通过同一身份隔离会话
桥转发：

```bash
export CORTEXFS_GMAIL_ACCESS_TOKEN='...'
export CORTEXFS_GMAIL_BIND='127.0.0.1:8767'
export CORTEXFS_GMAIL_PATH='/gmail/push'
cortexfs-channel gmail
```

Email 使用 IMAP IDLE 收件和 SMTP STARTTLS 回信。当前 host 处理明文文本与简单
RFC 5322 消息；在附件策略实现前不会暴露 MIME 附件上传/下载能力：

```bash
export CORTEXFS_EMAIL_IMAP_HOST='imap.example.org'
export CORTEXFS_EMAIL_SMTP_HOST='smtp.example.org'
export CORTEXFS_EMAIL_USERNAME='agent@example.org'
export CORTEXFS_EMAIL_PASSWORD='read-from-a-secret-store'
cortexfs-channel email
```

IRC 使用可重连的 TCP 客户端，支持 `PRIVMSG`、私聊与配置的频道加入。当前传输是
明文 IRC；若服务器要求机密性，请通过 TLS relay 或加密网络端点接入：

```bash
export CORTEXFS_IRC_SERVER='irc.example.org'
export CORTEXFS_IRC_NICKNAME='cortexfs-agent'
export CORTEXFS_IRC_CHANNELS='#agents'
cortexfs-channel irc
```

Signal 使用本地 `signal-cli` 进程边界。host 将 Signal 的协议状态保留在 message
ABI 之外，并重连 receive 进程：

```bash
export CORTEXFS_SIGNAL_ACCOUNT='+15551234567'
cortexfs-channel signal
```

Slack 与飞书/语雀采用显式 webhook 入站模式：

```bash
export CORTEXFS_CHANNEL_PLATFORM=slack
export CORTEXFS_CHANNEL_OUTBOUND_URL='https://slack.com/api/chat.postMessage'
export CORTEXFS_CHANNEL_BIND=127.0.0.1:8765
cortexfs-channel webhook
```

Slack 将 outbound URL 配置为 `https://slack.com/api/chat.postMessage`，并将
`CORTEXFS_CHANNEL_TOKEN` 设为 bot token。Feishu/Lark 使用 tenant 的
`im/v1/messages` endpoint，并按租户要求设置 bearer token。
当部署希望使用单个 URL 模板覆盖多个 codec 时，`CORTEXFS_CHANNEL_OUTBOUND_URL`
中的 `{path}` 会被替换为相对 API path。

除 Discord 外的凭据应置于服务管理器环境或 secret store 中。请勿将 token 写入
`/ctx`、仓库、命令行参数或持久会话元数据中。

## 多轮与 agent 能力

bridge 从通道、会话、线程，以及（内置多用户 host）外部发送方身份中派生一个稳定
会话，并使用 `scope=private` 向现有 `agent/<name>.sock` 提交文本，因此 agent 的
历史、上下文快照、工具调用、审批、子 agent 移交、取消与 provider 路由都与本地
`ctx agent send` 完全一致。

Discord 与 Telegram 前台 host 会立刻确认入站消息、创建有界的“thinking”占位符，
并将流式 delta 事件合并为平台编辑。完成后移除占位并显示失败反应与可见错误，
当 agent 或 provider 失败时亦如此。若某个平台操作不可用，host 降级为最终单条消息；
进度效果不会成为持久会话事实。

同一条入站消息会产生同一幂等键，因此 socket runtime 通过既有重放规则处理重试，
并继续作为 durable session facts 的唯一写入方。

WebSocket 前端是全双工：在初次 `input` 后仍可在首轮流式期间发送 `status`、
`resume`、`cancel` 或新的 `input`，并可在同一连接上应答运行时 `command`。host
会关联这些请求并在内部使用独立的 agent socket stream；因此浏览器与终端共享同一
interaction ABI。

## 覆盖范围边界

当前内置 host 覆盖 Telegram 长轮询、Discord Gateway、DingTalk Stream Mode、Matrix
Client-Server 同步、Slack Events/webhook、Feishu/Lark webhook、WhatsApp Business
Cloud webhook、Gmail Push、IMAP/SMTP 邮件、Signal（通过 `signal-cli`）、IRC、
Mattermost WebSocket/REST 与 QQ Bot Gateway/REST。独立 crate 暴露与平台无关的
message、lifecycle、capability、effect 与 socket ABI；其无状态 codec 覆盖上述所有原生
负载族。

ZeroClaw 的文档化通道集比内置 host 更广：iMessage 与其 CLI 通道仍需在此处新增原生
适配器。当前 Email、Gmail、Signal 与 IRC 实现同样存在上文显式限制；附件、签名、
E2EE 与平台特有流式行为需作为能力显式新增，而不能默默按纯文本处理。通用
`driver` 命令在迁移期间仍是第三方适配器的进程隔离扩展点。

## 从其他 agent 应用扩展

添加公共 crate 并为平台传输实现 `ChannelAdapter`：

```toml
cortexfs-channels = "0.1"
```

适配器可使用任意 HTTP/WebSocket/runtime 技术栈。它通过 `ChannelCapabilities`
 上报平台能力，通过返回 `DeliveryReceipt` 值并注册到 `ChannelRegistry`。若平台提供
webhook JSON，其无状态 codec 可实现 `ChannelCodec` 而不依赖 CortexFS。agent 应用仍
负责将入站会话绑定到自身 durable 会话。

规范合同见 [spec/channel-abi.md](spec/channel-abi.md)；crate 的 Rust API 为确切类型与
trait 签名的权威来源。
