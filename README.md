# CortexFS

**CortexFS is in a product reset.** No new product direction has been approved, and this checkout contains no buildable product implementation.

[简体中文](README.zh-CN.md) · [Project scope](PROJECT_SCOPE.md) · [Historical documentation](docs/README.md)

The previous Agent runtime, Linux FUSE /ctx interface, process sandbox, model/session system, channels, and MCP infrastructure are retired as active plans. Their documents remain for historical reference only; they are not a roadmap or implementation requirements.

## Current work

Only problem discovery and small feasibility tests are active. Before proposing code, identify one recurring user task, test existing tools, define a small first-use experience, and apply the stop/go gates in [PROJECT_SCOPE.md](PROJECT_SCOPE.md).

There are no current product build or installation commands. Do not use the old reconstruction recipe to infer the next product.

## Repository contents

- [PROJECT_SCOPE.md](PROJECT_SCOPE.md): active scope, non-goals, and decision gates.
- [docs/](docs/README.md): archived design and ABI documentation.
- [recipes/](recipes/cortexfs/RECIPE.md): historical reconstruction material for an earlier product snapshot.
- [AGENTS.md](AGENTS.md): contribution rules during the reset.

## Contribute

Keep changes small. Do not restore retired subsystems or add generic abstractions before a concrete problem is approved. Work on a branch and submit a pull request; do not push directly to main.

[MIT license](LICENSE)