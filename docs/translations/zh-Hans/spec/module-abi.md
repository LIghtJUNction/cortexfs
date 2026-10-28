# 模块 ABI

本文档定义了 CortexFS 运行时扩展的两个有意区分边界。实现由可独立使用的
`cortexfs-module` crate 提供。

Rust trait 是静态 module API，而非稳定的二进制 ABI。Rust 并未保证 trait-object、
vtable、`String`、allocator、panic 或 async future 布局在编译器版本、目标平台或动态库间
稳定。稳定的外部边界是下文定义的版本化 JSONL 套接线约定。

## 范围

module ABI 为 Agent、Tool、Channel、Model、Context 扩展提供统一的身份、能力与生命周期。
它是对标 Pi 的架构图中 agent-core 扩展边缘
（[architecture.md](../architecture.md)）：modules 接入生命周期与能力声明；
它们不发明根类，也不拥有前端。

```text
CortexFS Runtime
      |
      +-- cortexfs-module
            +-- Agent
            +-- Tool
            +-- Channel
            +-- Model
            +-- Context
```

它不定义新的 `/ctx` 类别、provider message 类型、会话存储、FUSE 操作或外部平台协议。
领域 SDK 仍负责自身的类型行为，运行时仍负责策略、路由、持久化与进程归属。

## 元数据与能力

每个 module 都暴露 `ModuleMetadata`：

```rust
ModuleMetadata::new("channel.example", "1.0.0", ModuleKind::Channel)
    .with_capability("text", "send and receive text")
```

`id` 与 `version` 标识实现；`kind` 为 `Agent`、`Tool`、`Channel`、`Model` 或 `Context`。
能力名是平台无关声明，不授予权限。是否可用某能力仍由策略和领域 ABI 决定。

静态 Rust 标识符为 `cortexfs.module/v1`。它版本化的是一次 Cargo 构建内的 typed host API，
并不保证从 `.so` 文件安全加载 Rust trait object。

## 生命周期

`CortexModule` 有四个与 executor 无关的异步操作：

```text
Registered -> Initialized -> Running -> Stopped -> Shutdown
```

host 仅提供包含其运行实例标识的 `ModuleContext`。此 ABI 下 module 不会接收 FUSE
handle、`/ctx` 路径、secret 或 Agent 回调。`ModuleRegistry` 按稳定 id 注册 module，并按
确定性 id 顺序驱动它们。

生命周期失败使用 `ModuleError`；host 可将其包裹入更广泛的运行时诊断系统，而不丢失 module
id 与操作边界。

## 外部进程 socket ABI

外部 module 是独立监管的进程，通过运行时拥有的 Unix socket 连接。该 socket 每帧携带一条
换行结束 JSON 对象，最大编码帧大小为 1 MiB。版本为 `cortexfs.module.socket/v1`；未知 JSON
字段出于前向兼容会被忽略，但未知 `type` 或错误的 `abi` 会被拒绝。

初始握手是 host 到 module 的 `hello`，随后是 module 到 host 的 `ready`：

```json
{"type":"hello","abi":"cortexfs.module.socket/v1","instance":"agent-1"}
{"type":"ready","metadata":{"id":"channel.example","version":"1.0.0","kind":"channel","capabilities":[]}}
```

host 驱动 `lifecycle` frame（`init`、`start`、`stop`、`shutdown`）。领域 SDK 使用 provider-
中立的 `call`/`result` frame，module 发布 `event` frame。`error` frame 包含稳定 code 与有界
诊断文本；frame 内不应包含 secret 或 prompt 内容。附件等大数据须使用既有 file/object ABI 并在
领域 payload 中引用，而不是放大 socket frame。

wire 合约是序列化与 framing ABI，并不承诺每个 module 可调用每个子系统。运行时策略、Linux
凭据、对象收据与现有 `/ctx`/会话 socket 权限仍由 host 持有。

## Runtime 与 Unix ABI 关系

module 是代码，不是文件系统对象。运行时状态继续使用现有文件与 socket：

```text
module code -> runtime 注册与生命周期
durable state -> 现有 agent/session 文件
live control/events -> 现有 agent/session socket
```

不会引入 `/ctx/module`、`/ctx/plugin` 或 `/ctx/channel` 命名空间。适配器可在现有 ABI 已允许处
暴露普通对象本地或会话本地状态。

## 加载边界

Rust trait 用于静态组合与 Cargo feature 选择。外部进程/socket 合约是推荐的第三方 module
扩展边界，因为它也提供了进程身份与故障隔离。未来原生 in-process loader 需采用独立 C ABI，
并使用 `#[repr(C)]`、不透明 handle、显式 ownership 函数与握手；不能暴露 Rust trait 对象。
WASM/WIT 是另一种可沙箱边界选择。无论采用何种传输，核心 module 元数据与生命周期模型不变。
