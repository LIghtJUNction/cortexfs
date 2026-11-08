# Historical documentation archive

[Project](../README.md) · [简体中文](translations/zh-Hans/README.md) · [Active scope](../PROJECT_SCOPE.md)

**Everything in this directory describes a retired CortexFS design or a historical source snapshot.** It is preserved for provenance and research. It is not the current product architecture, roadmap, or implementation contract.

The previous design combined an Agent runtime, Linux FUSE /ctx interface, process and network boundaries, model/session compatibility, channels, and MCP adaptation. That design is no longer active. The repository currently has no buildable product implementation.

## Archive map

| Historical subject | Reference |
| --- | --- |
| Product responsibility and old architecture | [Architecture](architecture.md), [Internal architecture](internal-architecture.md) |
| Old paths and wire formats | [ABI specification](spec/README.md) |
| Installation and packaging | [Getting started](getting-started.md), [Packaging](packaging.md) |
| Runtime usage and CLI | [Using CortexFS](using-cortexfs.md), [ctx](CTX.md), [agent.sh](agent-sh.md) |
| Channels and user interfaces | [Channels](channels.md), [Chat UI](chatui.md) |
| Extensions and development | [Development](developing-cortexfs.md), [Extensions](extensions.md) |
| Evaluation and recorded evidence | [Evaluation](evaluation.md), [Harness](harness.md), [Reports](reports/2026-09-05-harness-validation.md), [Benchmarks](benchmarks/20260714-agent-summary.json) |
| Documentation presentation | [Design guide](DESIGN.md) |

## Historical snapshots

| Source | Historical meaning |
| --- | --- |
| [84f349c](https://github.com/LIghtJUNction/cortexfs/tree/84f349c97bc2f6a611109686a02e21de3800042a) | Earlier product snapshot represented by the reconstruction recipe. |
| [7261cb3](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb) | Later documentation snapshot, including the hosted Agent CLI migration. |

Do not combine these snapshots or use either as the specification for a future product. A new design must start from an approved user problem and a minimal scope in [PROJECT_SCOPE.md](../PROJECT_SCOPE.md).