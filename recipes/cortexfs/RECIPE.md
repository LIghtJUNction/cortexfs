# CortexFS 重建配方（历史归档）

> **已退役：不要将本配方用于新产品实现。** 本配方描述早期 CortexFS Agent runtime 与 Linux /ctx 文件系统快照，只为历史复现和来源追溯保留。当前项目方向尚未确定；此配方不代表当前路线图或兼容性要求。

```toml
format = "recipkg/1"

[package]
name = "cortexfs"
version = "0.1.0"
kind = "software"
description = "历史重建：CortexFS v0.1.21 的 Linux /ctx 文件系统、Agent 运行时、CLI、适配器和双语文档快照。"
entry = "BUILD_PROMPT.md"

[dependencies.quality]
source = "../software-quality"

[dependencies.agent-shell]
source = "../cortexfs-agent-shell"
```

配方版本与产品版本分别管理。产品观察基线固定为
`LIghtJUNction/cortexfs@84f349c97bc2f6a611109686a02e21de3800042a`；
后来的 hosted Agent CLI 文档不属于本次历史重建目标。

`quality` 提供当时的工程、呈现和性能质量合同；`agent-shell` 是独立的可选薄客户端。
其余运行时、协议、通道适配器、SDK、安装与站点要求均属于该历史快照，不构成当前
产品的实现要求。

本配方中的目标平台、外部前提、证据、资产边界和未验证项分别见
`ENVIRONMENT.md`、`OBSERVATIONS.md`、`ASSETS.md`、`PERFORMANCE.md` 和
`ACCEPTANCE.md`。历史材料不得被描述为当前产品已构建或已通过验收。