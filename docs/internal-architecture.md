# CortexFS Internal Architecture

This document is the engineering-structure companion to
[architecture.md](architecture.md). Normative ABI lives under [spec/](spec/).
It defines Rust/process layering, allowed dependencies, error policy, and the
migration from the former self-hosted Agent runtime to a thin Unix/FUSE
execution boundary.

The design target is small: hosted Agent CLIs keep Agent intelligence;
CortexFS keeps Linux authority and inspectable filesystem/process primitives.
Pi remains a simplicity reference, not a feature checklist.

## 1. Internal objective

CortexFS should make it obvious where a change belongs:

```text
hosted CLI behavior      -> outside CortexFS core
launch/process policy    -> execution boundary
filesystem projection    -> FUSE layer
stable path/object ABI   -> ABI/foundation
external protocol bridge -> narrow adapter at the edge
legacy self-hosted loop  -> compatibility code being reduced
```

A new backend should normally require executable/argv selection and perhaps a
small launch profile. It must not require a new provider registry, Agent state
machine, session database, or core branch on the backend name.

## 2. Process architecture

The target runtime shape is ordinary Unix processes:

```text
frontend / user
      |
      v
ctx / supervisor
      |
      +-- derive uid/gid/groups + policy
      +-- build authorized mounts/sockets
      +-- derive network namespace / egress authority
      +-- apply resource ceilings
      +-- choose stdio or PTY
      |
      v
hosted Agent CLI
      |
      +-- authorized workspace
      +-- /ctx through FUSE
      +-- explicit MCP/tool/socket projections
```

Current process roles:

| Process | Owns | Must not own |
| --- | --- | --- |
| `cortexfs-mount` | FUSE projection and filesystem enforcement | provider calls or Agent loops |
| `ctx` | CLI control surface and launch requests | backend intelligence |
| `ctxterm` | PTY lifecycle, child process, attach/watch mechanics | tool/model/session semantics |
| root terminal broker | authenticated descriptor grants | PTY byte relay or Agent logic |
| `ctxmcp` | explicit MCP adaptation | a new `/ctx/mcp` root |
| channel adapter | one external platform transport | core Agent authority |
| hosted Agent CLI | its own loop/session/auth/approvals/tools | authority beyond projected Linux capabilities |

The existing `cortexfs-agent-runtime`, object-runner loop, and related session
machinery remain compatibility processes until issue #318 removes or narrows
them. New hosted-CLI work must not deepen those dependencies.

## 3. Backend-neutral launch contract

The internal process contract is:

```text
program + argv
cwd
environment
stdin/stdout/stderr or PTY
uid
gid
supplementary groups
umask / effective mode policy
authorized mounts and sockets
network namespace / egress authorization
resource limits
exit status
signals / cancellation
```

Identity, mount/socket authority, and network authority are not optional
metadata. The launcher derives them from the agent object and effective
CortexFS policy before spawning the child. A launch helper that merely inherits
the operator's identity or unrestricted host network does not satisfy this
contract. Default-deny policy therefore remains meaningful for provider-backed
CLIs instead of being bypassed at process launch.

Backend-specific flags stay in launch profiles/adapters. Core code should be
written against the process contract above, not `match "codex"` or equivalent.

## 4. Layer rules

Dependencies point downward only:

```text
application / bins / channels
        |
        v
execution / compatibility runtime
        |
        v
FUSE + protocol adapters
        |
        v
foundation ABI / paths / plain data / support
```

The foundation layer contains stable path/object ABI data types and other plain
contracts that may be shared by both FUSE and execution code. Runtime object
execution, policy orchestration, executors, launchers, and compatibility loops
belong above FUSE. This separation is what keeps `fuse -> executor` illegal
while still allowing FUSE to understand stable object/path schemas.

Concrete rules:

- foundation code may not import runtime, FUSE, or binaries;
- protocol conversion may not import Agent/session orchestration;
- FUSE may import only foundation object/path/support contracts; it may not
  import executors, launch/runtime orchestration, or command-line UI;
- execution/runtime code may depend on foundation contracts and compose FUSE
  through its public boundary, but FUSE never depends back on execution;
- binaries compose library layers; library code never imports `bin/*`;
- channel/platform crates stop at neutral socket/runtime-client boundaries;
- launch profiles may depend on generic process/ABI types but not on another
  backend profile;
- no `mod.rs`, no `unsafe`, and no new library `Result<_, String>`.

If a dependency edge exists only because the old self-hosted loop needs it,
that edge is a migration candidate rather than a precedent.

## 5. Crate direction

The intended gravity is:

```text
Foundation
  paths / stable object ABI types / plain data
  support: fs, jsonl, layout, process helpers
  module contract

FUSE + protocol boundary
  FUSE projection over foundation ABI
  protocol adapters over foundation/open contracts

Execution boundary
  object policy evaluation and compatibility runtime
  process launch / receipts / resources / network authority
  PTY + terminal broker

Edge adapters
  MCP
  channels
  thin hosted-CLI launch profiles
  telemetry / external protocol projections

Compatibility-only while migrating
  cortexfs-protocol provider/model IR
  provider registry / model projections
  Agent SDK loop
  object-runner model/tool loop
  CortexFS-owned compaction/session orchestration
  legacy interaction runtime where still consumed
```

Compatibility-only does not mean delete blindly. Before removal, search for
independent consumers, stable public API use, and ABI promises. If the only
consumer is the obsolete self-hosted runtime and a hosted CLI already owns the
behavior, prefer deletion over another abstraction layer.

## 6. FUSE and storage boundary

`/ctx` is read-write overall. Per-path Unix mode, uid/gid, mount semantics, and
CortexFS policy attenuate that access.

Never grant write access by binding the backing generation/storage tree around
FUSE. That would make permission checks observational instead of authoritative.
Writable Agent paths must remain writable through the FUSE boundary or another
explicitly authorized Unix object.

For explicit hosted commands using the canonical `/ctx` root, the launch
boundary now exposes one writable `/ctx` projection only after a per-start
condition confirms that the effective mount is the read-write CortexFS FUSE
filesystem. The legacy no-command `/ctx/bin/tsh` path and noncanonical/custom
roots remain read-only while migration continues. Backing storage is never
writable-bound around FUSE.

Atomic state updates use the repository's existing same-directory temporary
file plus rename convention. Do not add a second commit/control protocol.

## 7. Workspace semantics

A read-write workspace is a normal Unix subtree. Generic launch code does not
special-case `.git`, `.jj`, editor metadata, or backend-specific project files.
Explicit policy may narrow any subpath.

Linked worktrees are the important exception: if `.git` points to metadata
outside the authorized workspace, that external path is not automatically
included. It needs its own authorization and mount.

## 8. Environment and home

Environment construction is an authority boundary.

- pass only variables required by the launched CLI and policy;
- do not inherit provider secrets merely because they exist in the operator's
  shell;
- preserve XDG semantics when config/data/cache paths are intentionally
  projected;
- do not expose the full user home to make one CLI work;
- Omarchy/mise wrappers should be resolved through explicit executable/config
  projections, not by trusting all of `~/.local/bin`.

Provider/API credentials normally remain owned by the hosted CLI's existing
auth flow. CortexFS should not copy them into a second secret/session system.

## 9. PTY, stdio, signals, and lifetime

Interactive and headless execution share the same child-process authority.
Only I/O transport differs.

`ctxterm` already provides the reusable PTY child boundary. Hosted CLI support
should extend explicit program/argv selection around that machinery rather than
inventing a second runner.

Cancellation must reach the owned child/process group and produce an auditable
CortexFS boundary result. Exit status remains the child's ordinary process
status. Do not reinterpret backend failures into provider-specific core enums
unless a stable external contract requires it.

No background watcher, reverse-dial control plane, or hot-reload manager is
introduced. Git commit is the sole development/config activation boundary.
Process restart is ordinary lifecycle only and must not make uncommitted
development/config changes authoritative or activate them as an alternative to
a commit.

## 10. Sessions and durable facts

Hosted CLIs own their conversation/session formats unless CortexFS has a
separate ABI reason to persist something.

CortexFS may persist facts it owns:

- agent/object definition;
- effective policy and launch receipt;
- process state and exit status;
- terminal metadata and replay;
- explicit audit facts;
- compatibility session state required by existing stable APIs.

Do not create a universal conversation schema merely to normalize Codex,
Claude Code, Pi, and Antigravity. Preserve backend-native resume/session
behavior through the process boundary where possible.

## 11. Open protocols and adapters

Use standards at the edge:

- MCP remains MCP;
- OAuth/OIDC remain provider/CLI auth protocols;
- OpenTelemetry records boundary facts where useful;
- JSON-RPC/SSE/WebSocket stay transport/protocol choices of the external
  surface that already uses them.

An adapter is justified when it maps an existing external contract to an
existing CortexFS primitive. An adapter that invents a new private world model
is not.

## 12. Error policy

Errors should identify the boundary that failed:

```text
path/layout violation
identity/policy denial
mount projection failure
network-policy / namespace failure
process spawn failure
PTY/broker failure
resource-limit failure
signal/cancellation failure
external adapter/protocol failure
```

Library errors use narrow enums with `Display` and `Error`, preserving sources
when available. Process-local binaries may wrap those in an `ExecError`-style
shell. Do not add one error type per backend for the same Unix failure.

## 13. Testing contracts

Prefer behavior at the real boundary:

- fake executable receives exact argv/cwd/env;
- uid/gid/groups and umask/mode policy are derived correctly;
- stdio and PTY modes preserve exit status;
- signals/cancel terminate the owned child without orphans;
- unauthorized mounts fail closed;
- network authority is policy-derived: denied launches remain isolated and
  explicitly authorized egress does not inherit broader host network access;
- canonical explicit hosted commands receive writable `/ctx` only after the
  effective read-write CortexFS FUSE mount is validated at process start;
- `/ctx` rejects writes only where Unix/FUSE policy intentionally narrows them;
- read-write workspaces preserve normal `.git` semantics;
- out-of-tree linked-worktree metadata remains inaccessible without separate
  authorization;
- MCP/open-protocol adapters conform without backend-specific core branches.

Live tests against Codex/Claude/Pi/Antigravity are optional smoke tests when the
CLI is installed and no private credentials are required. Deterministic fake
executables remain the contract test.

## 14. Migration discipline

Issue #318 is incremental. Keep at most one active implementation slice when
possible.

Before each change:

1. search for an equivalent issue/PR/helper;
2. identify whether the target code has independent consumers;
3. ask whether deletion or direct Unix composition is simpler;
4. preserve stable root/path ABI;
5. add a boundary-level regression test;
6. record additions/deletions and conceptual surface.

A good migration PR removes more Agent-runtime concepts than it adds. A change
that introduces a manager, registry, provider branch, or private protocol needs
strong evidence that Unix primitives and public protocols cannot express the
requirement.

## 15. Current migration boundary

Completed:

- repository rules now define hosted CLIs as the target architecture;
- #320 removed the implicit `.git` mask from authorized read-write workspaces;
- #322 reused `ctxterm` and added exact hosted child program/argv selection;
- #325 gave explicit hosted children ordinary one-shot Unix process lifetime;
- #326 reduced `/ctx` to one effective root projection;
- canonical explicit hosted commands now guard writable `/ctx` at process start.

Next:

- carry policy-derived network authority through the generic launch boundary;
- project the minimum executable/runtime/config material for Omarchy/mise and
  generic Arch/Linux hosted CLIs without trusting full home or ambient PATH;
- complete fake-executable process-contract tests for cwd/env, stdio/PTY, exit,
  signals/cancel, identity, mounts/sockets, and network fail-closed behavior;
- retire legacy provider/model/tool/session runtime pieces as their consumers
  disappear.

Until migration completes, old self-hosted loop code is compatibility surface,
not the engineering target and not a pattern for new features.
