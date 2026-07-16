# CortexFS

[中文](README.zh-CN.md)

[![Pages deployment](https://img.shields.io/github/actions/workflow/status/LIghtJUNction/cortexfs/pages.yml?branch=main&label=pages)](https://github.com/LIghtJUNction/cortexfs/actions/workflows/pages.yml)
[![Documentation](https://img.shields.io/badge/docs-live-2A8F73)](https://lightjunction.github.io/cortexfs/)
[![crates.io](https://img.shields.io/crates/v/cortexfs)](https://crates.io/crates/cortexfs)

CortexFS FUSE-based runtime exposes agent operations through constrained `/ctx` filesystem ABI.

## Scope philosophy

- Keep stable root ABI small.
- Treat `/ctx/model`, `/ctx/agent`, `/ctx/tool`, `/ctx/home`, `/ctx/shared` as first-class object namespaces.
- Provider routing secret state and runtime internals, not root-ABI directories.
- Tool execution and agent runtime share one submission model: write temporary files then atomically rename to `.req.json`.

## Useful docs

- [Developing CortexFS](docs/developing-cortexfs.md)
- [CortexFS design notes](docs/DESIGN.md)
- [Architecture invariants](docs/architecture.md)
- [Internal architecture](docs/internal-architecture.md)

## External references

### Projects
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

- CortexFS PR:
  - [#89](https://github.com/LIghtJUNction/cortexfs/pull/89)
  - [#88](https://github.com/LIghtJUNction/cortexfs/pull/88)
  - [#87](https://github.com/LIghtJUNction/cortexfs/pull/87)
- MCP PR/Issue references moved to source comments:
  - `crates/cortexfs-tool-sdk/src/lib.rs`


### Spec references
- [Model Context Protocol (2025-06-18)](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)
- [Model Context Protocol (2025-03-26)](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports)
- [Linux FUSE documentation](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)
- [mount.fuse page](https://manpages.ubuntu.com/manpages/jammy/man8/mount.fuse.8.html)
- [MCP Security and Authorization](https://modelcontextprotocol.io/docs/tutorials/security/authorization)
