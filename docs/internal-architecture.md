# CortexFS internal architecture

[Documentation](README.md) · [Architecture](architecture.md) · [简体中文](translations/zh-Hans/internal-architecture.md)

> **Scope:** engineering contracts retained through
> [`7261cb3`](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb).
> Product source and tests are absent from this checkout. These contracts
> describe the hosted CLI migration; the earlier
> [reconstruction recipe](../recipes/cortexfs/RECIPE.md) has its own fixed baseline.

[Architecture](architecture.md) defines responsibilities and snapshot status.
This page specifies where code belongs and what its boundaries must enforce.
Detailed ABI rules live in the [specification](spec/README.md).

## Layer rules

Dependencies flow from composition and execution toward plain ABI contracts.

| Layer | Allowed responsibility and dependencies |
| --- | --- |
| Foundation | Stable path/object types, plain data, module contracts and common filesystem, JSONL, layout and process helpers; no FUSE, runtime or binary imports |
| FUSE | Projection and enforcement using foundation object/path/support contracts; no executor, launch orchestration or CLI imports |
| Protocol conversion | Conversion at an existing external boundary; no Agent/session orchestration imports |
| Execution | Policy, launch, receipts, resources, network, PTY and compatibility runtime; foundation contracts and FUSE's public boundary |
| Binaries and edge adapters | Compose library layers; channel adapters stop at neutral socket/runtime-client interfaces |

Library code never imports `bin/*`. Launch profiles may use generic process/ABI
types but cannot depend on another backend profile. The retained Rust rules
exclude `mod.rs`, `unsafe` and new library `Result<_, String>` APIs.

## Process architecture

The launcher derives authority before starting a child. These process roles
keep filesystem enforcement, transport and Agent behavior independently owned.

| Process | Owns |
| --- | --- |
| `cortexfs-mount` | FUSE projection and filesystem enforcement |
| `ctx` | CLI control surface and launch requests |
| `ctxterm` | PTY, child lifetime and attach/watch mechanics |
| Root terminal broker | Authenticated descriptor grants; no PTY byte relay |
| `ctxmcp` | Explicit MCP adaptation |
| Channel adapter | One external platform transport |
| Hosted Agent CLI | Its native model, auth, tool-loop, approval and session behavior |

FUSE, PTY and transport code do not own Agent orchestration. Compatibility
runtime dependencies are reviewed for removal as independent consumers
and stable API obligations disappear.

## Launch contract

Interactive and headless execution share this process contract:

| Area | Required inputs or results |
| --- | --- |
| Invocation | Executable, exact argv, cwd and explicit environment |
| I/O | stdin/stdout/stderr or PTY |
| Identity | UID, GID, supplementary groups and umask/effective mode policy |
| Reachable resources | Authorized mounts and sockets; policy-derived network namespace and egress |
| Limits | Resource/cgroup ceilings |
| Lifetime | Child exit status, signals and cancellation |

Derive identity, resource access and network authority from the agent object
and effective CortexFS policy **before spawn**. Do not inherit the operator's
identity, credentials or unrestricted network as defaults. Backend flags belong
in thin launch profiles using this contract.

## Filesystem and network enforcement

`/ctx` is read-write overall; Unix mode, identity, mount semantics and policy
narrow access per path. Every authorized write must pass through FUSE or another
explicitly authorized Unix object. Writable binds of backing generation/storage
trees are prohibited.

For an explicit hosted command at canonical `/ctx`, each launch must confirm
that the effective mount is a read-write CortexFS FUSE filesystem before
exposing one writable projection. The retained migration keeps legacy
no-command `/ctx/bin/tsh` launches and noncanonical/custom roots read-only.
See the [runtime contract](spec/agent-runtime.md).

An explicit hosted command keeps `--unshare-net` unless all three conditions
hold: policy allows `network:default connect`, `/usr/bin/pasta` is executable,
and `/dev/net/tun` is available. Missing or denied prerequisites preserve
isolation. The legacy no-command `tsh` path does not enable pasta. The
[network policy ABI](spec/tool-policy-abi.md) defines the full transport rules.

Read-write workspaces preserve ordinary `.git`, `.jj` and editor metadata
semantics. Explicit policy may narrow any subpath. Linked-worktree metadata
outside the authorized workspace needs separate authorization and a mount.

Atomic state updates use a temporary file in the same directory followed by
rename, subject to the relevant ABI's durability requirements. Keep one state
publication mechanism.

## Environment and home

The launch boundary constructs a curated environment with bwrap `--clearenv`
and `--setenv`; `ctxterm` forwards that result to the child. Pass only variables
and paths required by the CLI and policy. Ambient provider secrets, the full
user home and all of `~/.local/bin` are outside that default projection.

Preserve XDG config/data/cache semantics and resolve wrappers such as mise
through explicit executable/config paths. Provider credentials stay with the
hosted CLI's authentication flow. Do not duplicate them into CortexFS secrets
or conversation state.

The snapshot used Omarchy as a primary validation target. Its compatibility
work remains ordinary Arch/Linux work: command discovery, XDG, `systemd --user`,
Wayland terminal environment, FUSE permissions, identity/groups and PTY/signals.
Apply the same minimal projections on other Linux environments.

## Process lifetime and activation

Use `ctxterm` for PTY and child-process mechanics in the retained design.
Interactive and headless modes have the same authority; only I/O differs.
Cancellation must reach the owned child/process group, leave no orphan and
record the boundary result. Preserve the child's ordinary exit status.

Git commit is the sole development/config activation boundary. Restart may
consume a committed generation; it cannot activate uncommitted changes.
Do not add a background watcher, reverse-dial control plane or hot-reload
manager as another activation path.

## Durable facts and adapters

CortexFS persists facts it owns: agent/object definitions, effective policy,
launch receipts, process state and exit status, terminal metadata/replay,
audit facts and session state required by stable compatibility APIs.
Hosted CLIs retain their native conversation formats and resume behavior.

Record process starts/exits, signals, mount or identity denials, resource
failures, PTY attachment and FUSE denials at the boundary. A universal schema
for every backend's model/tool events requires a concrete external consumer.
Adapters map an existing external contract to a CortexFS primitive; protocol
choices follow the [extension points](architecture.md#extension-points).

## Error policy

Errors identify the failing boundary: path/layout, identity/policy, mount,
network/namespace, spawn, PTY/broker, resource limit, signal/cancellation or
external adapter/protocol. Rust libraries use narrow enums implementing
`Display` and `Error`, retaining source errors. Binaries may add an
`ExecError`-style wrapper. Equivalent Unix failures share a representation
across backends.

## Testing contracts

Use deterministic fake executables and boundary-level tests for these checks:

| Boundary | Required observation |
| --- | --- |
| Invocation and environment | Exact argv/cwd/env; only authorized variables and paths reach the child |
| Identity and limits | Correct UID/GID/groups, umask/mode policy and resource ceilings |
| Stdio and PTY | Child exit status survives both transports |
| Cancellation | Signals reach the owned process group without orphaned children |
| Mounts and FUSE | Unauthorized mounts fail closed; writable canonical `/ctx` requires a valid read-write FUSE mount on every launch |
| Path permissions | Writes follow per-path policy; read-write `.git` works; external worktree metadata needs separate authority |
| Network | Denied or missing prerequisites retain isolation; permitted egress stays within policy |
| Adapters | MCP/open-protocol conformance without backend-specific core branches |

Live CLI smoke tests are optional when the CLI is installed and private
credentials are unnecessary. They supplement the deterministic contract tests.
This checkout contains no runtime or test runner with which to execute them.

## Migration discipline

Before an implementation slice, check existing issues/helpers, independent
consumers, public API use and stable ABI obligations. Prefer direct Unix
composition or deletion when a hosted CLI already owns the behavior. Keep
one active slice where practical, preserve root/path compatibility, add a
boundary regression test and record the change in conceptual surface.

The [snapshot status](architecture.md#snapshot-status) records completed and
remaining work in the retained notes. New managers, registries, provider
branches or private protocols need a concrete requirement that existing
primitives cannot express.
