# CortexFS

<p align="center">
  <img src="docs/assets/cortexfs-hero.svg" alt="CortexFS /ctx" width="900">
</p>

<p align="center">
  <a href="https://crates.io/crates/cortexfs"><img alt="crates.io" src="https://img.shields.io/crates/v/cortexfs"></a>
  <a href="https://lightjunction.github.io/cortexfs/"><img alt="documentation" src="https://img.shields.io/badge/docs-live-2A8F73"></a>
  <a href="https://www.rust-lang.org/"><img alt="Rust 1.91+" src="https://img.shields.io/badge/rust-1.91%2B-000000?logo=rust"></a>
  <a href="https://github.com/LIghtJUNction/cortexfs/blob/main/LICENSE"><img alt="MIT" src="https://img.shields.io/badge/license-MIT-2A8F73"></a>
</p>

**A Unix/FUSE execution substrate for existing Agent CLIs.** CortexFS exposes a
small `/ctx` filesystem ABI and a policy-bound Linux process boundary so tools
such as OpenAI Codex CLI, Anthropic Claude Code, Pi, and Antigravity can run
without CortexFS reimplementing their model loop, provider auth, session logic,
context compaction, approval UX, or tool orchestration.

The hosted-CLI migration is tracked in
[#318](https://github.com/LIghtJUNction/cortexfs/issues/318). The target
architecture is normative, but generic launch profiles are still being migrated;
current releases may still contain compatibility paths from the former
self-hosted runtime.

```text
/ctx/model     compatibility model objects and inspectable facts
/ctx/agent     policy-bound execution principals
/ctx/tool      governed capability endpoints
/ctx/channel   channel state and channel-local tools
/ctx/home      per-UID durable state
/ctx/shared    explicitly shared state
```

## Core properties

- `/ctx` is a read-write FUSE filesystem overall. Individual paths may be
  read-only by Unix/FUSE policy; backing storage must never be exposed as a
  writable bypass around FUSE.
- Authority comes from Linux identity, mount/path visibility, network policy,
  sockets, resource limits, and CortexFS policy—never prompts, skills, or model
  output.
- Hosted Agent CLIs own provider/model authentication, Agent loops,
  session/context semantics, compaction, approvals, and CLI-native tools.
- CortexFS owns the execution ceiling: executable/argv, cwd/env, stdio or PTY,
  uid/gid/groups, umask/mode, authorized mounts/sockets, policy-derived network
  authority, resource limits, exit status, signals, and cancellation.
- Backend differences stay in thin launch profiles/adapters. Core code must not
  grow provider-specific Agent branches, registries, or a second wire protocol.
- Stable root/path ABI remains small and Unix-shaped. There is no parallel
  `workflow`, `mcp`, `skill`, `memory`, `chan`, or provider root.

Read the normative [specification](docs/spec/README.md),
[architecture](docs/architecture.md), and
[internal architecture](docs/internal-architecture.md) before production use.

## Hosted CLI direction

The first target hosted CLIs are:

- [OpenAI Codex CLI](https://github.com/openai/codex)
- Anthropic Claude Code
- [Pi](https://github.com/earendil-works/pi)
- Google Antigravity CLI as the current Google-side extension target

Pi is also a design reference for small cores and clear boundaries; it is not a
feature checklist. Omarchy is the primary desktop/development validation
environment, while implementations should remain ordinary Arch/Linux rather
than add Omarchy-specific branches in core.

CortexFS should reuse each CLI's existing non-interactive or interactive modes,
JSON/JSONL or RPC surfaces, resume/session behavior, MCP support, authentication,
and approval model. If a capability can be expressed as a Unix process contract
or an open protocol, CortexFS should not wrap it in a new private protocol.

## Tools, channels, and open protocols

`cortexfs-channels` defines the generic message, receipt, effect, command, and
`cortexfs.channel.socket/v1` boundary. Platform adapters own their credentials,
rate limits, retries, uploads, and transport lifetimes.

`cortexfs-channel-sdk` is the Rust SDK for a process-isolated adapter. See
[channels.md](docs/channels.md) and the
[Channel ABI](docs/spec/channel-abi.md).

For external tool servers, prefer MCP directly when MCP already fits. For local
execution, prefer Unix files, sockets, stdio/PTTY, process status, signals, and
mode/identity semantics. OAuth 2.0/OIDC belongs to hosted CLI/provider
authentication flows rather than a new CortexFS root or Agent framework.

## Compatibility runtime

The repository still contains compatibility code from the former self-hosted
Agent runtime, including `cortexfs-protocol`, model/provider adapters, Agent SDK
loop behavior, object-runner loops, compaction/session orchestration, and
interaction-runtime paths. These components remain only while real consumers
need them.

New work must not extend those components to match features already owned by
Codex, Claude Code, Pi, Antigravity, or another hosted CLI. Migration work should
prefer deletion, responsibility narrowing, and thin process adapters.

Legacy provider authentication commands and model objects may remain available
for compatibility, but they are not the target ownership model for new hosted
CLI integrations. Hosted CLIs should keep their own supported authentication
and provider behavior unless a concrete compatibility requirement proves a
CortexFS boundary is necessary.

## Evaluation

The optional `cortexfs-futureagi` adapter turns a validated CortexFS ATIF
trajectory into Future AGI evaluation inputs or submits it to a compatible
Future AGI endpoint. It is explicit and one-shot: no background uploader and
no additional filesystem ABI. See [Future AGI evaluation](docs/futureagi.md).

## Install and develop

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
ctx status
ctx update --ref main        # plan an immutable host update
ctx update --ref main --yes  # apply with native-package rollback
```

For a source checkout:

```bash
cargo run -p cortexfs --bin ctx -- bootstrap
cargo run -p cortexfs --bin ctx -- doctor
scripts/test.sh cargo test --workspace
```

Continue with [getting started](docs/getting-started.md),
[using CortexFS](docs/using-cortexfs.md),
[extending CortexFS](docs/developing-cortexfs.md), and the
[specification index](docs/spec/README.md).