# CortexFS architecture (historical)

[Documentation archive](README.md) · [简体中文](translations/zh-Hans/architecture.md) · [Current scope](../PROJECT_SCOPE.md)

> **Retired design — do not implement as the current product.** This page records the hosted Agent CLI design retained through [7261cb3](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb). It is kept for historical reference only. The project has no approved replacement architecture.

## Historical responsibility split

CortexFS gave an existing Agent CLI a Linux execution boundary. The CLI ran
as a process; its identity, visible paths, sockets, network and resource limits
came from the agent definition and host policy. The CLI kept its own model
interaction and conversation behavior.

| Component | Historical responsibility |
| --- | --- |
| Hosted Agent CLI | Model/provider selection and authentication, tool loop, approvals, sessions, context and compaction |
| CortexFS execution boundary | Identity, mounts, sockets, network policy, resources and child-process lifetime |
| FUSE /ctx | Inspectable objects and path-level read, write and execution permissions |
| Frontends and channels | User interaction and transport through authorized interfaces |

## Historical root ABI

The old root contract defined eight entries:

| Path | Historical purpose |
| --- | --- |
| /ctx/status | Mount status |
| /ctx/bin/ | ABI helper commands |
| /ctx/model/ | System model executables |
| /ctx/agent/ | System agent executables |
| /ctx/tool/ | System tool executables |
| /ctx/channel/ | Channel drivers, tools and capability state |
| /ctx/home/ | Per-user objects and state |
| /ctx/shared/ | Shared files for users and agents |

This ABI is retired with the design. It is not a compatibility requirement for the next product.

## Historical implementation boundaries

The old design separated FUSE projection, protocol conversion, execution, and edge adapters. It reused MCP, OAuth/OIDC, OpenTelemetry, and external JSON-RPC/SSE/WebSocket contracts where applicable. It also kept model authentication, approvals, and conversation behavior in the hosted CLI.

Those choices are records of the previous design, not a recommendation to restore them. See the [historical internal architecture](internal-architecture.md) and [ABI archive](spec/README.md) for provenance.

## Status

The product source is absent from this checkout. No old runtime, mount, process boundary, or adapter behavior has been rebuilt or validated here. New work must begin from an approved user problem, not by extending this architecture.