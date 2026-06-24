---
id: getting-started
title: Start With Installation
sidebar_label: Start With Installation
---

# Start With Installation

CortexFS is a Linux filesystem ABI. Install it first, confirm `/ctx` is usable,
then move on to agents, tools, and extension points.

## Install

Arch Linux users can install the AUR package directly:

```bash
paru -S cortexfs-git
sudo systemctl enable --now cortexfs.service
ctx doctor
ctx --help
```

`ctx doctor` should report whether the mount, base directories, default model
alias, and agent runtime state are available. `ctx --help` lists the
subcommands supported by the current build, including `ctx agent`, `ctx file`,
`ctx send`, `ctx exec`, and socket conveniences. The default live test does not
depend on an external cloud API.

## Meet `/ctx`

After installation, inspect the root:

```bash
ctx status
ctx ls
```

You should see a small stable set of entries:

```text
status
bin/
model/
agent/
tool/
home/
shared/
```

That is the core CortexFS tradeoff: the root keeps only stable object classes.
Provider, workflow, database, MCP registry, and skill registry details do not
become new top-level ABI entries.

## Set Shell Environment

Most commands infer these defaults automatically. Configure them explicitly
when needed:

```bash
eval "$(ctx env)"
```

This sets `CTX_ROOT`, `CTX_HOME`, `CTX_PATH`, and adds `/ctx/bin` to the normal
shell `PATH`. `CTX_PATH` is only for CortexFS tool lookup; it is not used to
find models or agents.

## First Object Checks

```bash
ctx ls model
ctx ls agent
ctx ls tool
ctx which model debug/echo
ctx which tool fs.read
ctx file classify tool/fs.read
```

`model/debug/echo` is the smallest debug model. It echoes input and is useful
for confirming that local installation and ABI paths work.

## Start An Agent Terminal

`ctx agent start` uses `systemd-run --user` to start `ctxterm -> tsh` inside a
bwrap sandbox. By default, it mounts the caller's current directory read-write
at `/workspace`:

```bash
ctx agent start coder --session default
ctx agent watch coder --session default
ctx agent attach coder --session default
```

`watch` only observes terminal output; `attach` joins the terminal and writes
stdin. Declare additional mounts explicitly:

```bash
ctx agent start coder --session docs \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

`tsh` is not a host shell. It only looks up CortexFS tools through `CTX_PATH`,
such as `/ctx/tool` and `/ctx/home/<uid>/tool`. `bash`, `tmux`, and `zellij`
must also be visible as tools before they can run.

## Next Step

Continue with [Daily Usage](using-cortexfs.md): `ctx`, `agent.sh`, shared
directories, and session history.
