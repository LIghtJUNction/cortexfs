---
id: developing-cortexfs
title: Extending CortexFS
sidebar_label: Extending CortexFS
---

# Extending CortexFS

Start with one rule: CortexFS extension points are the current spec's objects,
sockets, control files, and tool commit semantics. They are not new root
directories or new workflow entrances.

## Read The Boundary First

Suggested order:

```text
DESIGN.md
spec/README.md
spec/root-abi.md
spec/object-abi.md
spec/model-abi.md
spec/session-abi.md
spec/tool-policy-abi.md
spec/ctx-coreutils.md
aimock-testing.md
```

The root ABI only contains:

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
```

Do not add top-level directories such as `provider`, `workflow`, `job`, `hook`,
`mcp`, `skill`, or `audit`.

## Development Mental Model

CortexFS extension work starts with file operations, not framework integration:

```text
write agent/<name>.d/*     configure identity, model, authority, mounts, tool path
connect agent/<name>.sock  send JSONL conversation requests
execute tool/<name>        run a policy-bound capability
read session/*             inspect history, events, latest output, context packs
write *.req.json           commit async requests with atomic rename
append events.jsonl        persist runtime facts
```

A minimal agent runtime can be a single executable: read a request from stdin or
a socket, pick `agent/<name>.d/model`, and emit stable event frames. Richer
runtimes can add tool loops, context packing, child-agent orchestration, and
provider adaptation, but they still land on the same objects, sockets, and file
semantics.

## Extend Tools

A tool is an executable capability endpoint. Users see:

```text
/ctx/tool/<name>
/ctx/tool/<name>.d/
```

Execution can happen in the Rust runner, an external program, or runtime
internals, but authority is still decided by the agent view, `CTX_PATH`, and
policy.

For asynchronous tools or tools with retrievable results, use the unified
commit semantics:

```text
1. Write a temporary file.
2. Atomically rename it in the same directory to *.req.json.
3. Read results from outbox.
4. Append facts to audit.
```

This keeps tool development Unix-shaped. CLI mode uses argv/stdin/stdout; agent
native mode can use the tool SDK for structured JSON and in-process invocation.
Both modes share the same `.d/schema`, `.d/policy`, and visibility rules.

## Extend Agents

An agent is a policy-bound orchestrator. Stable paths are:

```text
/ctx/agent/<name>
/ctx/agent/<name>.sock
/ctx/agent/<name>.d/
/ctx/home/<uid>/agent/<name>/session/
```

Agents may organize tool loops, context, child tasks, and handoff, but those
orchestration concepts should not become new root ABI.

### Agent Tree

The base agent is the inheritable root identity. Child agents are not about
duplicating a process; they narrow the visible world:

```text
base
├── coder
│   └── reviewer
└── operator
```

A parent can create a child, but the child's model, tools, mounts, shared space,
uid/gid/groups, and policy must be a subset of the parent's authority. Child
handoff, result, refs, and lifecycle records live under the parent session's
`context/child/<id>/`. Owned children are cancelled with the parent task;
detached children require explicit policy.

### Terminal: ctxterm And tsh

The current `ctx agent start` terminal path is:

```text
systemd-run --user
bwrap sandbox
ctxterm
tsh
```

By default, it mounts the caller's current directory at `/workspace` inside the
sandbox. Extra mounts must be declared with `--mount SOURCE TARGET ro|rw`;
`TARGET` must not replace `/` or `/ctx`. This path is the agent terminal
implementation, not a new background watcher, polling loop, or hot-reload
subcommand.

`ctxterm` owns the PTY and exposes `watch` and `attach` through the session
terminal socket:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

`tsh` only looks up tools through `CTX_PATH`; it does not fall back to the host
`PATH`. If `CTX_PATH` is unset, it may read `CTX_HOME/.tshrc`, but that file
only supports data-form `CTX_PATH=...`.

The split is deliberate:

```text
ctxterm  owns PTY lifetime, watch/attach, and multi-observer terminal access
tsh      discovers tools, loads/pins them, and invokes capabilities via CTX_PATH
bash     is only a normal tool, available when visible and allowed
tmux     is also a normal tool, useful for long-running panes or background work
```

The default native tool visible to an agent is `tsh`. Additional tools do not
appear just because a prompt mentions them; they enter the working set through
`tsh tools`, `tsh load TOOL`, `tsh pin TOOL`, and `tsh TOOL ARG...`.

### Context Window Management

CortexFS treats context as a working set, not the source of truth:

```text
messages.jsonl     durable conversation facts
events.jsonl       durable runtime facts
latest.md          recent-output view, rebuildable
context/pack.md    current working set, rebuildable
context/refs.jsonl selected files, child results, search results
```

Prompt construction merges agent instruction, AGENTS.md rules, skill metadata,
tool injection, message history, and the runtime contract. Skill metadata starts
with `name`, `description`, and `SKILL.md path`. It may use at most 2% of the
context window; when the window size is unknown, the hard cap is 8,000
characters. Over-budget descriptions are shortened first, then some skills are
omitted with a warning. Full `SKILL.md` content is read only after a skill is
selected.

### Authority Control

Prompts and schemas are not the authority system. Effective authority is always
the intersection of several layers:

```text
mount/chroot visibility
Linux uid/gid/groups and mode bits
CortexFS label + policy v0
CTX_PATH tool visibility
tool executable metadata
noexec mount placement
```

For example, reading a file does not imply executing its related tool; seeing a
tool file does not imply policy allows invoking it; a prompt that says "you may
use shell" cannot bypass `tsh` or policy.

## Extend Providers Or Local Models

The provider/model design must stay neutral. CortexFS does not make any vendor
a core default path, and it does not make Ollama a core special branch.

The lightweight local live-test fixture uses:

```text
smollm2:135m
```

If that model is missing, tell the user to install or pull it; do not silently
switch models. When a user explicitly asks to test their configured provider or
aggregation API, use the existing provider registry, routes, secret state, and
unified commit semantics.

Provider API key resolution order is fixed:

```text
1. environment variable named by provider config
2. system keychain, for example service=cortexfs:<provider> account=default
3. unconfigured, return a stable error
```

Do not write secrets into `/ctx/model/*`, `.d/default`, or any other ABI file.

When you need to test an OpenAI-compatible provider path without calling a
cloud API, use this repository's aimock fixture:

```bash
npm install
npm run aimock
npm run aimock:smoke
```

See [AIMock Testing](aimock-testing.md) for details. This is a local test
fixture, not a new `/ctx/provider` root namespace.

The multi-API compatibility boundary is:

```text
/ctx/model/main                    stable default model alias
/ctx/model/<provider>/<model>      model objects projected by provider adapters
model/<name>.d/driver              driver/route metadata
provider registry/cache/keychain   runtime state, not root ABI
```

When switching providers, users update a model alias or route. Agents can keep
saying "use model:main". Provider compatibility does not leak into the agent,
tool, session, or authority model.

## Performance Design

CortexFS is efficient because the boundary is small:

```text
object discovery   directory reads and short control files
model/tool exec    file exec or Unix sockets
conversation       JSONL frame streams
context packing    durable history plus rebuildable working sets
tool context       explicit load/pin; unpinned entries reclaimed by W-TinyLFU
authority checks   static mount/policy/mode-bit intersection
```

The root ABI has only a few object classes, so providers, databases, workflows,
MCP servers, and temporary jobs do not each become new directories. Agent
runtimes can keep fast in-memory projections of visible tools, while durable
state remains plain files and stable events.

## Local Verification

Common checks:

```bash
cargo test
npm --prefix docs-site run build
```

The fixed FUSE integration test mount point is:

```text
tests/mounts/cortexfs
```

This directory is only a local test mount point. Do not put source, fixtures,
or persistent data there.
