# Documentation

[Project](../README.md) · [简体中文](translations/zh-Hans/README.md) · [Specification](spec/README.md)

CortexFS documents a Linux filesystem interface for agent execution. This
checkout contains the documentation and reconstruction recipes; the product
and website implementations are absent. Use the recipe for reconstruction and
the historical guides to inspect recorded behavior.

## Work with this repository

| Task | Entry point |
| --- | --- |
| Prepare a reconstruction | [Recipe](../recipes/cortexfs/RECIPE.md), [environment](../recipes/cortexfs/ENVIRONMENT.md), [build task](../recipes/cortexfs/BUILD_PROMPT.md) |
| Define verification | [Acceptance](../recipes/cortexfs/ACCEPTANCE.md), [performance](../recipes/cortexfs/PERFORMANCE.md), [media](../recipes/cortexfs/ASSETS.md) |
| Review proposed changes | [Research evidence](../recipes/cortexfs/RESEARCH.md), [improvement requirements](../recipes/cortexfs/IMPROVEMENTS.md) |

## Understand the design

| Question | Read |
| --- | --- |
| What does CortexFS own? | [Architecture](architecture.md) |
| How are processes and permissions bounded? | [Internal architecture](internal-architecture.md) |
| Which paths and wire formats are stable? | [ABI specification](spec/README.md) |
| How are paths and objects named? | [Path guide](paths.md), [naming guide](naming-guide.md) |
| How should documentation look and read? | [Documentation design](DESIGN.md) |

## Historical guides

These pages describe removed implementations and versioned behavior. Their
commands require a matching historical source tree or an independently built
product. Read each page's scope before using an example.

| Task | Guides |
| --- | --- |
| Install or package | [Installation](getting-started.md), [Linux packages](packaging.md) |
| Operate the runtime | [Daily usage](using-cortexfs.md), [`ctx`](CTX.md), [`agent.sh`](agent-sh.md) |
| Connect a frontend | [Channels](channels.md), [Chat UI](chatui.md) |
| Extend behavior | [Development](developing-cortexfs.md), [Extensions](extensions.md) |
| Evaluate behavior | [Evaluation](evaluation.md), [Harness](harness.md), [aimock](aimock-testing.md), [FutureAGI](futureagi.md) |
| Inspect recorded evidence | [Harness report](reports/2026-09-05-harness-validation.md), [benchmark data](benchmarks/20260714-agent-summary.json), [Verus proof scope](proofs/verus.md) |

## Versions and authority

| Source | Use |
| --- | --- |
| [`84f349c`](https://github.com/LIghtJUNction/cortexfs/tree/84f349c97bc2f6a611109686a02e21de3800042a) | Reconstruction baseline (`v0.1.21-10-g84f349c9`). The recipe's [bundled contracts](../recipes/cortexfs/contracts/docs/README.md) define that target. |
| [`7261cb3`](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb) | Last upstream source snapshot represented by `docs/`; it includes the later hosted CLI migration. |

Within `docs/`, the detailed English [specification](spec/README.md) takes
precedence over summaries and older translations. For the current checkout's
working rules, read [AGENTS.md](../AGENTS.md). Recipe validation and historical
reports have their own stated scope; fresh product results require fresh
execution evidence.

[Simplified Chinese](translations/zh-Hans/README.md) links to available
translations. Pages without a translation link to the English original.
