# CortexFS 重建配方

```toml
format = "recipkg/1"

[package]
name = "cortexfs"
version = "0.1.0"
kind = "software"
description = "重建 CortexFS v0.1.21 快照的 Linux /ctx 文件系统、Agent 运行时、CLI、适配器和双语文档体验。"
entry = "BUILD_PROMPT.md"

[dependencies.quality]
source = "../software-quality"

[dependencies.agent-shell]
source = "../cortexfs-agent-shell"
```

配方版本与产品版本分别管理。产品观察基线固定为
`LIghtJUNction/cortexfs@84f349c97bc2f6a611109686a02e21de3800042a`；
后来的 hosted Agent CLI 文档不属于本次重建目标。

`quality` 提供工程、呈现和性能质量合同；`agent-shell` 重建独立的可选薄客户端。
两者的职责、接口和集成验收见 `BUILD_PROMPT.md`。其余运行时、协议、通道适配器、
SDK、安装与站点作为本软件的组成部分整体重建，其完整要求在本包内，不以原始源码
仓库或语言库作为配方依赖。

优先按外部合同选择适合 Linux 的实现语言，可更换原 Rust/Bash/TypeScript 技术栈；
保留 Linux ABI 与英中双语行为。目标平台、外部前提、证据、资产边界和未验证项分别见
`ENVIRONMENT.md`、`OBSERVATIONS.md`、`ASSETS.md`、`PERFORMANCE.md`、`ACCEPTANCE.md`。

所有负载为 UTF-8 Markdown。图片、视频、实现、安装脚本、技能子模块与 Git 元数据
不属于配方；媒体的单独获取前提在 `ASSETS.md` 明示。内容许可见 `LICENSE.md`。
