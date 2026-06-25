---
id: using-cortexfs
title: Daily Usage
sidebar_label: Daily Usage
---

# Daily Usage

The everyday CortexFS experience should feel like Unix: discover objects, read
state, then execute files or connect to sockets when you need work done.

## Find Available Objects

```bash
ctx ls model
ctx ls agent
ctx ls tool
```

Common object shapes:

```text
/ctx/model/main
/ctx/model/debug/echo
/ctx/agent/coder
/ctx/agent/coder.sock
/ctx/tool/fs.read
```

The named file executes work, the matching `.sock` handles stateful JSONL
interaction, and the matching `.d/` directory stores small control files.

## Call A Model Directly

Start with the echo model while debugging:

```bash
/ctx/model/debug/echo "hello cortex"
echo "summarize this file" | /ctx/model/main
```

Use the proxy debug model when you want to inspect agent prompts without
configuring a provider:

```bash
/ctx/model/debug/proxy "explain what this agent can see"
```

By default it emits a portable proxy request that can be copied into any AI chat
surface. If the host tool exposes a CLI, set `CORTEXFS_PROXY_COMMAND`; then
`debug/proxy` writes the request JSON to that command's stdin and uses stdout as
the model response. This is a local debug bridge, not provider configuration.

Change the `/ctx/model/main` alias when you want a different default model.
Do not add provider-specific root entries.
Provider secrets are not written into model files or `.d/` control
directories; provider adapters resolve API keys in environment, system
keychain, then unconfigured order.

## Manage Agents

`ctx agent` is the thin client for the current ABI. Creating, starting, and
stopping agents still goes through ordinary tools or file ABI; it does not add
a workflow entrance:

```bash
ctx agent new reviewer --model openai/gpt-4o --tool fs.read
ctx agent new reviewer --label reviewer_t --shared project-a:read --mount /work /work ro
ctx agent start reviewer --session default
ctx agent status reviewer
ctx agent ps
ctx agent stop reviewer
```

If `/ctx/tool/agent.create`, `agent.start`, or `agent.stop` is missing, the
matching lifecycle command fails with service unavailable. `ctx agent status`
and `ctx agent ps` only read ordinary `agent/<name>.d/*` control files.

## Submit Images And Other Files

For images, PDFs, audio, archives, or other binary material, submit a path
reference instead of putting bytes into the prompt. CortexFS keeps the file
itself in workspace or shared space visible to the agent; the conversation only
describes the task and the path.

When you start an agent from the current directory, that directory is mounted as
`/workspace` by default:

```bash
ctx agent start coder --session default
ctx send coder "Analyze /workspace/assets/screenshot.png and summarize UI issues"
```

Use explicit mounts when you need tighter visibility:

```bash
ctx agent start coder --session image-review \
  --no-default-workspace \
  --mount "$PWD/assets" /input ro \
  --mount "$PWD/docs" /docs ro \
  --cwd /docs

ctx send coder --session image-review "Inspect /input/screenshot.png and use /docs/DESIGN.md"
```

Use shared space when multiple agents or sessions need the same material:

```bash
mkdir -p "$(ctx path shared project-a)/input"
cp screenshot.png "$(ctx path shared project-a)/input/"
ctx agent new reviewer --shared project-a:read
ctx send reviewer "Inspect /ctx/shared/project-a/input/screenshot.png"
```

This keeps large files out of message history. Context records paths,
summaries, and refs; reading image bytes, extracting text, rendering thumbnails,
or calling a vision model happens lazily through a visible tool.

## Watch And Attach Terminals

`ctx agent start` mounts the caller's current directory at `/workspace` inside
the sandbox by default, then starts `ctxterm -> tsh` from `/workspace`:

```bash
ctx agent start coder --session default
ctx agent watch coder --session default
ctx agent attach coder --session default
```

The terminal socket lives at:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

The FUSE-visible path may be a symlink to
`/run/cortexfs/terminal/.../main.sock`. `watch` is read-only; `attach` connects
your stdin to the terminal.

Control the sandbox explicitly when needed:

```bash
ctx agent start coder --session review \
  --no-default-workspace \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

## Use The Tool Shell

`tsh` is the CortexFS tool shell, not a host shell. It resolves commands in
this order:

```text
1. process CTX_PATH
2. CTX_HOME/.tshrc line CTX_PATH=...
3. default /ctx/tool:/ctx/home/<uid>/tool
```

`.tshrc` is a data file, not shell syntax:

```text
CTX_PATH=/ctx/tool:/ctx/home/1000/tool
```

Useful checks:

```bash
tsh --list
tsh which fs.read
tsh help fs.read
```

## Use agent.sh

The repository still includes `agent.sh` as a shell frontend:

```bash
install -m 0755 agent.sh/agent.sh ~/.local/bin/agent.sh
agent.sh --help
agent.sh coder
agent.sh coder "summarize this repository"
agent.sh --chat coder
agent.sh --attach coder
agent.sh --watch coder
agent.sh --session default coder "inspect the failing test"
agent.sh --resume coder
```

`agent.sh coder` opens the chat REPL through `ctx agent-sh coder`. With prompt
arguments, `ctx agent-sh` forwards one message to `ctx agent send`. Use
`agent.sh --watch coder` to observe the agent terminal, and `agent.sh --attach
coder` only when you want to enter `ctxterm -> tsh`. `agent.sh` does not keep a
private chat database.

## Use Shared Space

Shared space is an ordinary file directory. Use it for project material, task
input, and results exchanged between agents:

```bash
ctx path shared project-a
cd "$(ctx path shared project-a)"
```

Whether an agent can read or write a shared directory is decided by its view,
mounts, policy, Linux uid/gid, and mode bits.

## Inspect History

```bash
ctx agent history coder
ctx agent output coder
```

Without `--session`, these commands use `session/index/current` first and fall
back to `default`.

The underlying history lives at:

```text
/ctx/home/<uid>/agent/<agent>/session/
```

Raw history is durable fact; context is a rebuildable working set. Compacting
context must not destroy raw messages.
