# ctx Coreutils

`ctx` is the thin userland client for the `/ctx` ABI.

```text
ctx = CortexFS coreutils
```

It is not an AI chat product. It is not a daemon. It is not a runtime. It is
not a provider SDK. Its first job is to prove that the CortexFS ABI is usable
with ordinary Unix-shaped operations.

`ctx` operates on `/ctx`. It does not have to live inside `/ctx`. Install it in
the normal system `PATH`, for example `/usr/bin/ctx` or `~/.local/bin/ctx`.

## v1 Commands

Keep v1 small:

```text
ctx status
ctx abi
ctx env
ctx root
ctx bootstrap
ctx mount

ctx ls
ctx ls model
ctx ls agent
ctx ls tool
ctx ls home
ctx ls shared/project-a

ctx which model openai/gpt-4o
ctx which agent coder
ctx which tool fs.read

ctx path shared project-a
ctx history coder
ctx latest coder
ctx resume coder default

ctx agent new reviewer --model openai/gpt-4o --tool fs.read
ctx agent new reviewer --label reviewer_t --shared project-a:read --mount /work /work ro
ctx agent start reviewer
ctx agent stop reviewer
ctx agent status reviewer
ctx agent ps

ctx file cat agent/coder.d/policy
ctx file set agent/coder.d/cwd /work
ctx file append agent/coder.d/path /ctx/tool
ctx file check agent/coder.d/mount
ctx file classify tool/fs.read

ctx validate-name coder
ctx doctor
```

Do not add:

```text
ctx provider
ctx mcp registry
ctx workflow
ctx memory
ctx vector
ctx cluster
```

Socket conveniences such as `ctx send`, `ctx chat`, `ctx connect`, `ctx ping`,
and `ctx cancel` may exist, but they must be thin wrappers over the same socket
ABI.

Agent lifecycle conveniences exist as thin wrappers:

```text
ctx agent new NAME [--temp] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]
ctx agent start NAME
ctx agent stop NAME
ctx agent status NAME
ctx agent ps
```

`ctx agent new`, `ctx agent start`, and `ctx agent stop` must call
`/ctx/tool/agent.create`, `/ctx/tool/agent.start`, and `/ctx/tool/agent.stop`
respectively. If those tools are absent, the commands fail with service
unavailable. `ctx agent new --temp` passes `life=temp` to `agent.create`; `ctx`
must not decide lifecycle policy locally. `ctx agent status`
may read `agent/<name>.d/status` directly because it is ordinary ABI
inspection. `ctx agent ps` may read `agent/<name>.d/parent`, `status`, and
`pid` directly and print the current agent tree.

## Installation Boundary

Reserve `/ctx/bin` for CortexFS ABI-level helper programs needed by runtimes,
chroots, or scripts:

```text
/ctx/bin/ctx
/ctx/bin/ctxterm
/ctx/bin/tsh
```

The first implementation may expose only `ctx`, but agent terminal runtimes
should use `ctxterm` and `tsh` when present. The placement rule is:

```text
human CLI              system PATH, usually one ctx binary
agent capability       /ctx/tool
runtime ABI helper     /ctx/bin
```

`ctxterm` is the agent terminal emulator. It owns the pseudo-terminal and starts
`tsh` by default. `tsh` is the tool shell that runs inside that terminal. `tsh`
resolves command names through `CTX_PATH`, not `PATH`, and must not execute
arbitrary host commands directly. A command such as `bash` works only when a
tool named `bash` is visible through `CTX_PATH`.

When `CTX_PATH` is not set, `tsh` reads `CTX_HOME/.tshrc` if it exists. The
file is data-only and supports a single stable setting:

```text
CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool
```

Do not let `/ctx/bin` become a second `/usr/bin`.

## Path Model

`ctx` resolves paths under `CTX_ROOT`, defaulting to `/ctx`.

Examples:

```text
ctx ls agent
ctx file cat model/openai/gpt-4o.d/cap
ctx file classify tool/fs.read
ctx exec agent/coder "fix tests"
```

Object strings use ABI path form:

```text
model/openai/gpt-4o
agent/coder
tool/fs.read
```

## Core Commands

`ctx status` reads `/ctx/status`.

`ctx ls` uses `readdir`. It accepts an ABI path under `CTX_ROOT`, defaulting to
the root when no path is provided. It does not query a database, index,
registry, or daemon catalog.

`ctx which` finds executable objects by ABI class:

```text
ctx which model openai/gpt-4o
ctx which agent coder
ctx which tool fs.read
```

`ctx file cat` reads ABI files. It should not interpret much.

`ctx file set` updates by same-directory atomic replacement. `ctx file append`
is only for appendable ABI files such as newline lists. `ctx file check`
validates path shape and file syntax where the ABI defines it.

`ctx file` classifies CortexFS paths using path shape, `stat`, `readlink`, and
existing control files. It does not query a registry.

Stable type strings:

```text
ctx.model.exec
ctx.model.socket
ctx.model.control
ctx.agent.exec
ctx.agent.socket
ctx.agent.control
ctx.tool.exec
ctx.tool.socket
ctx.tool.control
ctx.session.dir
ctx.session.messages
ctx.session.events
ctx.shared.dir
ctx.shared.tool.exec
ctx.shared.tool.control
ctx.shared.queue
ctx.shared.result
ctx.home.dir
ctx.symlink
ctx.ordinary
ctx.unknown
```

## cd

An external process cannot change its parent shell cwd. `ctx cd` must not
pretend otherwise.

Correct ordinary usage:

```bash
cd "$(ctx path shared project-a)"
```

If `ctx cd` exists, it is a shell integration helper:

```bash
eval "$(ctx cd project-a --shell)"
```

## Sessions

`ctx history`, `ctx latest`, and `ctx resume` read session files and connect to
the relevant socket. They do not keep a private chat database.

Examples:

```text
ctx history coder
ctx latest coder
ctx resume coder default
```

These commands read:

```text
/ctx/home/<uid>/agent/<agent>/session/index/list
/ctx/home/<uid>/agent/<agent>/session/index/current
/ctx/home/<uid>/agent/<agent>/session/<session>/latest.md
/ctx/home/<uid>/agent/<agent>/session/<session>/messages.jsonl
```

## Non-Goals

`ctx` must not:

```text
manage provider keys
store private chat history
implement tool calling
parse OpenAI/Anthropic/Gemini request formats
maintain an agent registry
modify messages.jsonl to fake chat
decide policy locally
fallback to another model
hide runtime errors behind product language
```

Those jobs belong to Rig, the agent runtime, tools, or the CortexFS ABI itself.
