# CortexFS

**CortexFS is in a product reset.** No new product direction has been approved, and this repository contains no buildable product implementation.

[简体中文](README.zh-CN.md) · [Project scope](PROJECT_SCOPE.md) · [Historical tree](https://github.com/LIghtJUNction/cortexfs/tree/9ffe10f2905ab13de6e8e2401ae38d499724f2b8)

The previous Agent runtime, Linux FUSE /ctx interface, process sandbox, model/session system, channels, and MCP infrastructure are retired. Their implementation plans and specifications have been removed from the current tree; Git history preserves the previous work.

## Current work

Only problem discovery and small feasibility tests are active. Before proposing code, identify one recurring user task, test existing tools, define a small first-use experience, and apply the stop/go gates in [PROJECT_SCOPE.md](PROJECT_SCOPE.md).

There are no product build or installation commands. Do not reconstruct the old product.

## Repository contents

- [PROJECT_SCOPE.md](PROJECT_SCOPE.md): active scope, non-goals, and decision gates.
- [AGENTS.md](AGENTS.md): contribution rules during the reset.
- [LICENSE](LICENSE): project license.

## Contribute

Keep changes small. Do not restore retired subsystems or add generic abstractions before a concrete problem is approved. Work on a branch and submit a pull request; do not push directly to main.

[MIT license](LICENSE)