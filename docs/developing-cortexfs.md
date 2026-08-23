---
id: developing-cortexfs
title: Extending CortexFS
sidebar_label: Extending CortexFS
---

# Extending CortexFS

Start with one rule: CortexFS extension points are the current spec's objects,
sockets, control files, and tool commit semantics. They are not new root
directories or new workflow entrances. That anti-framework placement matches
the Pi elegance bar in [architecture.md](architecture.md).

For the common case, start with the short path in [One-file Extensions](extensions.md):
put tools and executable agents in one package directory, describe them in one
`cortexfs.toml`, run `ctx install --check ./package`, then install with
`ctx install ./package`. The package is only an authoring convenience; installation still uses the same hash-bound atomic
object publication and the same `agent/<name>.d/*` / `tool/<name>.d/*` ABI.

## Read The Boundary First

Suggested order:

```text
DESIGN.md
architecture.md          # Pi-aligned elegance bar and extension points
internal-architecture.md # crate/module layer rules
spec/README.md
spec/root-abi.md
spec/object-abi.md
spec/model-abi.md
spec/module-abi.md
spec/session-abi.md
spec/tool-policy-abi.md
spec/ctx-coreutils.md
extensions.md
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

CortexFS extension work starts with file operations, not framework integration.
But these files are not always disk files.

A path under `/ctx` may be backed by disk, or it may be a memory projection
derived from the current agent, session, authority, and context. Traditional
agent architectures often expose extra debug APIs, dump JSON, or repeatedly
write runtime state to files just so developers can inspect context. Disk files
add I/O and synchronization cost; tmpfs is fast but ephemeral. FUSE lets those
states appear as files: if nobody opens, stats, or reads a path, it does not
need to be materialized; when inspection is needed, ordinary Unix tools work.

That is the core shape of CortexFS: hidden runtime state becomes a
what-you-see-is-what-you-get file view, while remaining deeply customizable. An
agent does not need a new framework; it only needs the high-level objects:
files, sockets, executable tools, and sessions.

```text
write agent/<name>.d/*     configure identity, model, authority, mounts, tool path
connect agent/<name>.sock  send JSONL conversation requests
execute tool/<name>        run a policy-bound capability
read session/*             inspect history, events, latest output, context packs
read context/*             inspect working sets, file refs, child results
read xattr/stat            inspect file type, origin, token estimate, security facts
```

A minimal agent runtime can be a single executable: read a request from stdin or
a socket, pick `agent/<name>.d/model`, and emit stable event frames. Richer
runtimes can add tool loops, context packing, child-agent orchestration, and
provider adaptation, but they still land on the same objects, sockets, and file
semantics.

For images, PDFs, audio, archives, and other non-text inputs, do not stuff bytes
into the prompt and do not invent a separate upload API. Put the file somewhere
visible to the agent, then reference the path in the conversation:

```bash
ctx agent start coder --session default --mount "$PWD" /workspace rw
ctx send coder "Analyze /workspace/assets/screenshot.png and compare it with /workspace/docs/DESIGN.md"
```

For material shared across agents or sessions, use shared space:

```bash
mkdir -p "$(ctx path shared project-a)/input"
cp screenshot.png "$(ctx path shared project-a)/input/"
ctx agent new reviewer --shared project-a:read
ctx send reviewer "Inspect /ctx/shared/project-a/input/screenshot.png"
```

The runtime only needs to record those paths in `context/refs.jsonl` or the
context pack. Reading image bytes, estimating tokens, rendering thumbnails, or
calling a vision model should happen lazily through the relevant tool or
provider adapter.

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

In some deployments, `/ctx/agent/<name>.sock` is an owner-authorized symlink
into the user runtime path (for example `/run/user/<uid>/cortexfs/agent/...`),
and in some deployments it may be a direct socket node. Probe the live mount
before assuming a single implementation form.

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
`PATH`. Standalone human sessions read `CTX_HOME/.tshrc` before inherited
process `CTX_PATH`, and that file only supports data-form `CTX_PATH=...`.
Inside an agent terminal, the runtime-provided `CTX_PATH` remains
authoritative.

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

The agent or another userspace runtime selects content, constructs the pack, and
writes `context/pack.json` and `context/pack.md` by same-directory atomic
replacement. CortexFS owns pack shape and source validation, `/ctx` visibility,
and file durability; it does not select prompts, estimate budgets, or rebuild
packs for the runtime.

This is a 0.2.0-class breaking API retirement: the public
`rebuild_context_pack`, `ContextPackBuildError`, `ContextPackBuild`, and
`ContextPackBuiltItem` symbols, including their associated methods, are removed.
Userspace writers may still validate their outputs with
`inspect_context_pack_json` and `validate_context_pack_source`.

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

Provider API key resolution is:

```text
1. provider environment candidates (if present)
2. root-owned CortexFS system secret store
3. unconfigured, return a stable error
```


Do not write secrets into `/ctx/model/*`, `.d/default`, or any other ABI file.
OAuth access tokens follow the same principle: provider adapters read secret state from the system secret store.
Provider configuration may declare Authorization Code + PKCE metadata.
By default, the access token is stored under `service=cortexfs:<provider> account=oauth:access`, and refresh token under
`account=oauth:refresh`. PKCE verifier, state, access token, refresh token
must not be written into `/ctx/model/*`, `.d/default`, or any other ABI file.
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
provider registry/cache/secret store   runtime state, not root ABI
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
scripts/test.sh cargo test
npm --prefix docs-site run build
```

`scripts/test.sh` runs the test command with a private `/tmp` tmpfs (2 GiB
by default), so test fixtures cannot fill the host temporary filesystem. Set
`CORTEXFS_TEST_TMPFS_BYTES` to a decimal byte limit when a larger bounded test
scratch space is required.

The fixed FUSE integration test mount point is:

```text
tests/mounts/cortexfs
```

This directory is only a local test mount point. Do not put source, fixtures,
or persistent data there.

## Reference Projects and Similar Code (keyword checks)

- [tursodatabase/agentfs](https://github.com/tursodatabase/agentfs)
- [modelcontextprotocol filesystem server](https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem)
- [rust-mcp-stack/rust-mcp-filesystem](https://github.com/rust-mcp-stack/rust-mcp-filesystem)
- [opencrust multi-agent runtime](https://github.com/opencrust-org/opencrust)

### Related issue / PR

- CortexFS
  - [#89](https://github.com/LIghtJUNction/cortexfs/pull/89)
  - [#88](https://github.com/LIghtJUNction/cortexfs/pull/88)
  - [#87](https://github.com/LIghtJUNction/cortexfs/pull/87)
- modelcontextprotocol/filesystem server
  - [#3232](https://github.com/modelcontextprotocol/servers/issues/3232)
  - [#3402](https://github.com/modelcontextprotocol/servers/issues/3402)
  - [#4208](https://github.com/modelcontextprotocol/servers/issues/4208)

### Similar code-search keywords

- `provider registry` + `object` + `policy`
- `Fuse` + `socket runtime` + `jsonl`
- `atomic rename .req.json` + `outbox` + `audit append`
- `model alias` + `route` + `secret store`
