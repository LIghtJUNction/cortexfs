# CortexFS

[English](README.en.md)

[![Pages deployment](https://img.shields.io/github/actions/workflow/status/LIghtJUNction/cortexfs/pages.yml?branch=main&label=pages)](https://github.com/LIghtJUNction/cortexfs/actions/workflows/pages.yml)
[![Documentation](https://img.shields.io/badge/docs-live-2A8F73)](https://lightjunction.github.io/cortexfs/)
[![crates.io](https://img.shields.io/crates/v/cortexfs)](https://crates.io/crates/cortexfs)

CortexFS 是一个通过受限 `/ctx` 文件系统 ABI 暴露 agent 的 FUSE 运行时。

## 快速开始

在使用 systemd，且已启用仓库能提供所需软件包的当前 Arch、Debian/Ubuntu、
Fedora/RHEL 或 openSUSE/SLES 系 Linux 上运行：

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
```

安装器会在本机使用 `Cargo.lock` 构建下载的源码快照，并先审计 systemd、
FUSE、bubblewrap 0.10+ 与 Rust。每个持久化变更都会展示计划，只有输入精确确认
指令后才会执行。重复运行只更新二进制和 unit，不覆盖数据、密钥、provider 配置
或现有环境文件。首次成功安装会默认跟随系统语言，并提供可跳过的 AI 接入引导。

安装完成后可运行 `ctx status`、`ctx ls`、`ctx doctor` 和 `ctx --help`。

## 项目介绍

<p align="center">
  <a href="https://www.youtube.com/watch?v=3BhCiHWbrUQ">
    <img src="https://img.youtube.com/vi/3BhCiHWbrUQ/maxresdefault.jpg" alt="观看 CortexFS 项目介绍视频" width="720">
  </a>
</p>

<p align="center">
  <a href="https://www.youtube.com/watch?v=3BhCiHWbrUQ">在 YouTube 观看 CortexFS 介绍视频</a>
</p>

## 范围与原则

- 保持稳定的 root ABI 足够小。
- `/ctx/model`、`/ctx/agent`、`/ctx/tool`、`/ctx/home`、`/ctx/shared` 作为一等命名空间。
- 模型路由与密钥状态属于运行时内部实现，不写入 root ABI。
- 工具与模型执行共享同一提交语义：先写临时文件，再原子重命名为 `.req.json`。

## 文档入口

- [CortexFS 开发指引](docs/developing-cortexfs.md)
- [CortexFS 设计说明](docs/DESIGN.md)
- [架构约束](docs/architecture.md)
- [内部架构](docs/internal-architecture.md)

## 外部参考

### 同类项目
- [tursodatabase/agentfs](https://github.com/tursodatabase/agentfs)
- [j0hanz/filesystem-mcp](https://github.com/j0hanz/filesystem-mcp)
- [modelcontextprotocol/filesystem server](https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem)
- [rust-mcp-stack/rust-mcp-filesystem](https://github.com/rust-mcp-stack/rust-mcp-filesystem)
- [mark3labs/mcp-filesystem-server](https://github.com/mark3labs/mcp-filesystem-server)
- [cyanheads/filesystem-mcp-server](https://github.com/cyanheads/filesystem-mcp-server)
- [TexasFortress-AI/rs_filesystem](https://github.com/TexasFortress-AI/rs_filesystem)
- [colinrozzi/fs-mcp-server](https://github.com/colinrozzi/fs-mcp-server)
- [corporatepiyush/mcp-filesystem-rust](https://github.com/corporatepiyush/mcp-filesystem-rust)
- [rawr-ai/mcp-filesystem](https://github.com/rawr-ai/mcp-filesystem)
- [safurrier/mcp-filesystem](https://github.com/safurrier/mcp-filesystem)
- [SylphxAI/filesystem-mcp](https://github.com/SylphxAI/filesystem-mcp)
- [QuantGeekDev/mcp-filesystem](https://github.com/QuantGeekDev/mcp-filesystem)
- [efforthye/fast-filesystem-mcp](https://github.com/efforthye/fast-filesystem-mcp)
- [lileeei/sand-mcp-fs](https://github.com/lileeei/sand-mcp-fs)
- [proofmath-owner/ai-filesystem-mcp](https://github.com/proofmath-owner/ai-filesystem-mcp)
- [github/github-mcp-server](https://github.com/github/github-mcp-server)
- [conikeec/mcpr](https://github.com/conikeec/mcpr)
- [strawgate/filesystem-operations-mcp](https://github.com/strawgate/filesystem-operations-mcp)
- [webconsulting/mcp-server-wsl-filesystem](https://github.com/webconsulting/mcp-server-wsl-filesystem)
- [avelino/mcp](https://github.com/avelino/mcp)
- [wonker007/surgicalfs-mcpserver](https://github.com/wonker007/surgicalfs-mcpserver)

### CortexFS MCP

- CortexFS PR：
  - [#89](https://github.com/LIghtJUNction/cortexfs/pull/89)
  - [#88](https://github.com/LIghtJUNction/cortexfs/pull/88)
  - [#87](https://github.com/LIghtJUNction/cortexfs/pull/87)
- MCP PR/Issue 设计讨论已移入源代码注释：
  - `crates/cortexfs-tool-sdk/src/lib.rs`

### 参考文档
- [Model Context Protocol 规范（2025-06-18）](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)
- [Model Context Protocol 规范（2025-03-26）](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports)
- [Linux FUSE 文档](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)
- [FUSE 挂载手册](https://manpages.ubuntu.com/manpages/jammy/man8/mount.fuse.8.html)
- [MCP 安全与授权](https://modelcontextprotocol.io/docs/tutorials/security/authorization)
