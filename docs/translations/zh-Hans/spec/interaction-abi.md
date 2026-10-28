# 交互 ABI

`cortexfs-runtime-client` 定义了 terminal、web 与 channel frontend 共享的 provider 无关交互约定。
它是逻辑协议；当前传输是现有的 agent/session Unix socket 及其兼容 `send`、`resume`、`status`
与 `cancel` 操作。

版本标记为 `cortexfs.interaction/v1`。一帧定义为一个换行结尾的 JSON：

```json
{
  "abi": "cortexfs.interaction/v1",
  "payload": {
    "direction": "request",
    "value": {
      "type": "input",
      "request_id": "web-1",
      "session": "default",
      "scope": "private",
      "input": "hello",
      "origin": {"transport": "web"}
    }
  }
}
```

请求覆盖 input、重放、状态查询、取消，以及对运行时发起命令的回复。事件标准化包括
accept、start、delta、message、tool、approval command、status、error 与 completion。
每个事件携带前端 request id，并在适用时附带 run id。运行时命令以 `command_result` 回答，
这使得双向都可独立关联，但不会让前端直接调用 tool。

Unix-socket 客户端除了受限事件读取器外保留一个写入句柄。当 `approval_request` 被标准化为
`command` event 时，客户端可以在读取下一个事件前写入该响应：

```json
{
  "abi": "cortexfs.interaction/v1",
  "payload": {
    "direction": "request",
    "value": {
      "type": "command_result",
      "request_id": "web-1",
      "session": "default",
      "command_id": "call-1",
      "result": {"type": "accepted"}
    }
  }
}
```

`InteractionOrigin` 被设计为泛化模型。它可以携带 transport、endpoint、外部身份、
conversation、thread 与有界 metadata，但不定义 Telegram、Discord、HTTP 或 provider 特定的
消息类型。身份解析与权限校验仍由运行时负责。

内置 web host 接收并返回这些精确 frame 作为 JSONL。浏览器客户端因此与 `ctxchat` 与通道
bridge 使用同一 request/event 模型；只有外层 HTTP 连接不同。由于当前 endpoint 是单次
HTTP POST，它会发送 command event 并对交互命令返回有界原因；要支持浏览器侧的 approval/input
回复，需要 WebSocket 或双向 NDJSON endpoint。

## 双层协议

interaction ABI 是前端/运行时层：

```text
terminal / web / IM
        |  cortexfs.interaction/v1
        v
agent session runtime
```

`cortexfs-channels` 独立定义 `cortexfs.channel.socket/v1` 作为 channel driver/runtime 边界。
它携带通道生命周期、入站事件、消息投递、关联 effect（typing、reaction、edit、delete、mark
read）、回执、健康与重连事件：

```text
platform adapter
        |  cortexfs.channel.socket/v1
        v
channel runtime -- interaction ABI --> agent session
```

平台 codec 位于该边界下方，将原生 payload 翻译为既有 `InboundMessage`/`OutboundMessage`
ABI，并且不会进入 Agent 代码。

这两套协议都不会创建 `/ctx/interaction` 或 `/ctx/channel` 命名空间。持久历史仍保留在既有会话路径；
socket 只是 live transport，文件仍是可观测状态面。

上述 Rust trait 是编译时 API，而非承诺稳定 Rust 二进制 ABI。外部实现应使用文档化 JSONL socket
frame，或独立版本化的可执行进程。
