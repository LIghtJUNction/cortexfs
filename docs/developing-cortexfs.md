---
id: developing-cortexfs
title: Extending CortexFS
sidebar_label: Extending CortexFS
---

# Extending CortexFS

Start with one rule: CortexFS is a Unix/FUSE execution substrate, not a second
Agent framework. Extend stable file, socket, process, policy, channel, and tool
boundaries; do not add a new root namespace, provider-specific Agent branch, or
another resident orchestration layer.

Read these first:

```text
AGENTS.md
architecture.md
internal-architecture.md
spec/README.md
spec/root-abi.md
spec/object-abi.md
spec/agent-tool-security.md
spec/tool-policy-abi.md
spec/terminal-abi.md
spec/terminal-broker.md
extensions.md
```

The frozen root is:

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/channel
/ctx/home
/ctx/shared
```

Do not add aliases or parallel roots such as `provider`, `workflow`, `job`,
`hook`, `mcp`, `skill`, `memory`, `chan`, `audit`, or `control`.

## Development mental model

CortexFS owns authority and execution boundaries. Hosted Agent CLIs own their
model/provider authentication, Agent loop, session/context semantics,
compaction, approval UX, and CLI-native tool behavior.

The backend-neutral launch contract is deliberately process-shaped:

```text
executable + argv
cwd + environment
stdin/stdout/stderr or PTY
uid + gid + supplementary groups
umask/mode policy
authorized mounts and sockets
policy-derived network namespace / egress authority
resource ceilings
exit status
signals + cancellation
```

If Codex CLI, Claude Code, Pi, Antigravity, or another CLI already exposes a
working text/JSON/JSONL/RPC/resume/MCP surface, call that surface directly.
Do not normalize it into a new CortexFS wire protocol unless a concrete external
consumer proves that a mapping is necessary.

## `/ctx` and filesystem authority

The host FUSE filesystem is read-write overall. Individual files or directories
may be read-only by Unix/FUSE policy. Backing storage must never be exposed as a
writable bind that lets a hosted CLI bypass FUSE enforcement.

Explicit hosted commands using the canonical `/ctx` root receive a single
writable `/ctx` projection only after process-start validation confirms the
effective mount is the read-write CortexFS FUSE filesystem. The legacy
no-command `/ctx/bin/tsh` path and noncanonical/custom roots remain read-only
during migration. Tests must not assume writable `/ctx` outside that guarded
hosted-command boundary.

A read-write `/workspace` follows normal Unix subtree semantics. CortexFS does
not implicitly hide `.git`; explicit path policy may still make
`/workspace/.git` read-only. Linked-worktree metadata outside the authorized
workspace needs separate authority.

## Hosted Agent CLIs

The first target hosted CLIs are OpenAI Codex CLI, Anthropic Claude Code, and
`earendil-works/pi`; Antigravity CLI is the current Google-side extension target.
These are target integrations while #318 is in progress, not a claim that every
launch profile already ships.

A backend adapter should usually be only a launch profile:

```text
program + argv spelling
required explicit environment
required config/executable mounts
interactive vs headless stdio/PTTY choice
```

It must not become a model router, provider registry, session manager, Agent
loop, auth broker, or workflow engine.

Omarchy is the primary desktop/development validation environment. Keep support
ordinary Arch/Linux: handle XDG paths, mise wrappers, systemd --user, Wayland
terminal context, FUSE, uid/gid/groups, signals, and PTY behavior without
blindly trusting the whole home directory or all of `~/.local/bin`.

## Tools and MCP

A CortexFS tool is an executable capability endpoint. Authority is the
intersection of visibility, Linux identity/mode, mount/path policy, and CortexFS
policy.

Prefer MCP directly for external tool servers where MCP already fits. `ctxmcp`
is an adapter, not a `/ctx/mcp` root and not a reason to copy MCP semantics into
core.

For asynchronous tools with durable results, keep the existing Unix-shaped
commit pattern:

```text
1. write a temporary request file
2. atomically rename it in the same directory to the committed name
3. read results from the defined outbox/result path
4. append audit facts where the ABI requires them
```

Prompt text, AGENTS.md, CLAUDE.md, skills, and tool schemas may influence CLI
behavior but never grant extra authority.

## Agent objects and launch

An Agent object is a policy-bound execution principal, not a CortexFS-owned AI
loop. Stable compatibility paths include:

```text
/ctx/agent/<name>
/ctx/agent/<name>.sock
/ctx/agent/<name>.d/
/ctx/home/<uid>/agent/<name>/session/
```

Some of these paths still serve the legacy runtime and remain compatibility
surface until migration removes or narrows them.

The current interactive path reuses ordinary process machinery:

```text
systemd-run --user
bwrap sandbox
ctxterm
child process
```

`ctxterm` owns PTY mechanics, attach/watch, child lifetime, and exit status. It
is not an Agent runtime. Hosted commands reuse this machinery through exact
child program/argv selection rather than a second runner or backend registry.

`tsh` remains a compatibility tool shell. New hosted CLI integrations should not
require CortexFS to absorb the CLI's native model/session/tool loop into `tsh`.

## Sessions, context, and durable facts

CortexFS may persist facts it actually owns: process receipts/status, terminal
replay, policy/audit facts, object definitions, and compatibility session data
that is still required by current ABI consumers.

Hosted CLIs remain authoritative for their own session/context formats. Do not
mirror every backend conversation or model/tool event into a universal CortexFS
session schema merely for symmetry.

Existing compatibility files such as `messages.jsonl`, `events.jsonl`, and
context packs may remain while real consumers exist. New work should narrow or
retire them when the hosted CLI already owns the equivalent state.

## Providers and authentication

Provider/model adapters, provider registry state, CortexFS auth profiles, and
`cortexfs-protocol` remain compatibility code for existing consumers. They are
not the ownership model for new hosted CLI integrations.

For Codex, Claude Code, Pi, Antigravity, and future hosted CLIs, prefer the
CLI's supported authentication and provider behavior. Only introduce a CortexFS
credential or provider boundary when a concrete integration requirement cannot
be expressed safely through the CLI and Unix/Open protocols.

Secrets must never be projected through `/ctx`, broad inherited environments,
or logs merely to make a CLI start.

## Extension packages

[One-file Extensions](extensions.md) remain an authoring convenience for
existing object/tool compatibility surfaces. Installation must still use the
same validated, hash-bound, atomic publication rules; a package must not become
a second `/ctx` namespace or a hot-reload plugin control plane.

Legacy executable Agent SDK packages are compatibility surface. Do not create
new Agent-loop features there when an external hosted CLI can own the behavior.

## Verification

Common checks:

```bash
scripts/test.sh cargo test --workspace
npm --prefix docs-site run build
```

For process-boundary changes prefer fixture/fake executables and verify:

```text
exact argv
cwd and explicit environment
stdio/PTY behavior
uid/gid/groups and umask
mount/socket visibility
network default-deny and authorized egress
exit status
signal/cancel/timeout behavior
no orphan child process
```

Only run live hosted-CLI smoke tests when the official CLI is actually available
and no sensitive credential exposure is required. State clearly which checks
were fixture-based and which were live.

## Common pitfalls

- **Do not invent a second Agent runtime.** If an external CLI already owns the
  model/tool/session loop, keep CortexFS at the process/FUSE boundary.
- **Do not inherit operator authority.** Identity, mounts, sockets, network, and
  resources derive from the Agent object plus CortexFS policy before spawn.
- **Do not bypass FUSE.** Writable backing-store binds are not an acceptable way
  to make `/ctx` writable to a hosted CLI.
- **Git commit is the development/config activation boundary.** Process restart
  is lifecycle only and must not activate uncommitted development/config changes.
- **No backend enum in core.** Backend differences stay in thin launch profiles.
- **No `mod.rs`; no `unsafe`.** Follow repository naming, layer, source-budget,
  and dependency rules in `AGENTS.md`.
- **`fuse` must not depend on executors.** FUSE may use foundation object/path
  ABI types, not launch/runtime orchestration.
- **Docs translations must not contradict canonical docs.** Remove a stale
  translation if it cannot be kept semantically aligned; locale fallback is
  safer than publishing the retired architecture.