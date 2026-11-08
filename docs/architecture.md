# CortexFS architecture

[Documentation](README.md) · [简体中文](translations/zh-Hans/architecture.md)

> **Scope:** the hosted Agent CLI design retained through
> [`7261cb3`](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb).
> This repository contains documentation and recipes; product source has been
> removed. The [reconstruction recipe](../recipes/cortexfs/RECIPE.md) uses the
> earlier `84f349c` contract snapshot.

## Responsibility split

CortexFS gives an existing Agent CLI a Linux execution boundary. The CLI runs
as a process; its identity, visible paths, sockets, network and resource limits
come from the agent definition and host policy. The CLI keeps its own model
interaction and conversation behavior.

| Component | Responsibility |
| --- | --- |
| Hosted Agent CLI | Model/provider selection and authentication, tool loop, approvals, sessions, context and compaction |
| CortexFS execution boundary | Identity, mounts, sockets, network policy, resources and child-process lifetime |
| FUSE `/ctx` | Inspectable objects and path-level read, write and execution permissions |
| Frontends and channels | User interaction and transport through authorized interfaces |

Interactive and headless clients use the same authority checks. Their I/O may
use stdio or a PTY; the [launch contract](internal-architecture.md#launch-contract)
defines the shared inputs and results.

## Frozen root rule

The root has exactly eight entries. The [root ABI](spec/root-abi.md) defines
their detailed structure and compatibility rules.

| Path | Purpose |
| --- | --- |
| `/ctx/status` | Mount status |
| `/ctx/bin/` | ABI helper commands |
| `/ctx/model/` | System model executables |
| `/ctx/agent/` | System agent executables |
| `/ctx/tool/` | System tool executables |
| `/ctx/channel/` | Channel drivers, tools and capability state |
| `/ctx/home/` | Per-user objects and state |
| `/ctx/shared/` | Shared files for users and agents |

New capabilities fit existing objects or external adapters. Skills, project
rules, prompts and CLI configuration remain ordinary visible files. Root names
such as `skill/`, `memory/`, `mcp/`, `workflow/`, `chan/`, `job/`, `hook/`,
`audit/` and `control/` are outside this ABI.

## Authority model

A process can act only through resources the host exposes and permits. CortexFS
combines Linux identity and mode bits with path policy, authorized mounts and
sockets, network rules and resource ceilings. Prompt text and project rules
can guide the CLI's decisions; they grant no additional authority.

`/ctx` is read-write overall, with access narrowed per path. Authorized writes
must pass through FUSE enforcement. Exposing writable backing storage would
bypass that check and is prohibited.

An authorized read-write workspace follows normal Unix semantics, including
its `.git` directory. Explicit policy can narrow a subpath. A linked worktree
whose Git metadata is outside the workspace needs a separate authorization
and mount. Launch-time FUSE checks and network isolation are specified under
[filesystem and network enforcement](internal-architecture.md#filesystem-and-network-enforcement).

## Extension points

Keep each adapter tied to an existing process or protocol boundary.

| Need | Mechanism |
| --- | --- |
| Another Agent CLI | Executable/argv selection and a thin launch profile |
| Tool server | MCP or the executable tool ABI |
| Platform channel | A process-isolated transport adapter |
| Project guidance | Visible rule and skill files |
| Additional hard authority | Unix/FUSE policy with explicit ABI review |
| Observability | Boundary facts and standard telemetry |

Hosted CLIs retain their native authentication, resume, approval and protocol
behavior. Reuse MCP, OAuth/OIDC, OpenTelemetry and external JSON-RPC/SSE/WebSocket
contracts where they apply. `agent.sh` stays a thin convenience client; backend
profiles adapt command-line spelling without adding a provider registry or a
second session engine.

## Snapshot status

The retained design names Codex CLI, Claude Code and Pi as initial targets,
with Antigravity as a further target. Launch profiles were still migrating
under [#318](https://github.com/LIghtJUNction/cortexfs/issues/318).

The `7261cb3` notes record these milestones:

| Reference | Recorded change |
| --- | --- |
| [#320](https://github.com/LIghtJUNction/cortexfs/pull/320) | Removed the implicit `.git` mask from authorized read-write workspaces |
| [#322](https://github.com/LIghtJUNction/cortexfs/pull/322), [#325](https://github.com/LIghtJUNction/cortexfs/pull/325) | Reused `ctxterm` for explicit program/argv selection and one-shot child lifetime |
| [#326](https://github.com/LIghtJUNction/cortexfs/pull/326), [#328](https://github.com/LIghtJUNction/cortexfs/pull/328) | Reduced `/ctx` to one effective projection and forwarded the curated launch environment |
| [#330](https://github.com/LIghtJUNction/cortexfs/pull/330) | Gated hosted pasta egress on policy and host prerequisites |

Remaining work in that snapshot included minimal executable/config projections,
complete process-boundary tests and removal of legacy runtime pieces after
checking their independent consumers. Legacy provider/model adapters, Agent SDK
and object-runner loops, compaction, session and interaction machinery are
historical compatibility subjects. Their source is absent from this checkout.
Product execution and migration completion require separate verification.

## Source of truth

- [Specification](spec/README.md): normative paths, formats and behavior.
- [Internal architecture](internal-architecture.md): layering, launch enforcement and verification contracts.
- [Repository instructions](../AGENTS.md): scope and contribution rules.
- [Documentation design](DESIGN.md): presentation and navigation.
