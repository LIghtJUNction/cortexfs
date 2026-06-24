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
tsh fs.read '{"path":"README.md"}'
```

## Use agent.sh

The repository still includes `agent.sh` as a shell frontend:

```bash
install -m 0755 agent.sh/agent.sh ~/.local/bin/agent.sh
agent.sh --help
agent.sh coder "summarize this repository"
agent.sh --session default coder "inspect the failing test"
agent.sh --resume coder
agent.sh --latest coder
```

`agent.sh` is a thin client. It reads and writes
`/ctx/agent/<agent>.sock` and session files; it does not keep a private chat
database.

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
ctx history coder
ctx latest coder
```

The underlying history lives at:

```text
/ctx/home/<uid>/agent/<agent>/session/
```

Raw history is durable fact; context is a rebuildable working set. Compacting
context must not destroy raw messages.
