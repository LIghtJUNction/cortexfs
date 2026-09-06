---
id: getting-started
title: Start With Installation
sidebar_label: Start With Installation
---

# Start With Installation

CortexFS is a Linux filesystem ABI. Install it first, confirm `/ctx` is usable,
then move on to agents, tools, and extension points.

## Install

The one-command installer supports current Arch-, Debian/Ubuntu-, Fedora/RHEL-,
and openSUSE/SLES-family Linux distributions booted with systemd when their
enabled repositories supply the required packages:

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
```

For native `.deb`, `.rpm`, Arch Linux, or portable tar packages, see the
[Linux packages guide](packaging.md).

To connect an agent to Telegram, Discord, Slack, or Feishu/Lark, see the
[multi-IM channel guide](channels.md). The guide uses the existing agent socket
and durable session ABI, so each conversation remains multi-turn.

It checks the distribution, systemd, FUSE, bubblewrap 0.10+, and Rust MSRV
before building the downloaded source snapshot locally with `Cargo.lock`.
Every persistent mutation has an exact typed confirmation. Re-running is
idempotent: program and unit files are updated while storage, secrets, provider
configuration, existing environment files, and `/ctx` user state are
preserved. A genuine first install chooses a language from the system locale
and offers optional OpenAI, Codex, Anthropic, Google, OpenRouter, DeepSeek,
Groq, or OpenAI-compatible onboarding through the existing provider commands.
It can also print an IM host template through `cortexfs-channel list|show|preset`.

Arch users may alternatively install `cortexfs-git` from the AUR, enable
`cortexfs.service`, and run `ctx doctor`.

`ctx doctor` should report whether the mount, base directories, default model
alias, and agent runtime state are available. `ctx --help` lists the
subcommands supported by the current build. The default live test does not
depend on an external cloud API.

## Update The Host

Plan against a branch, tag, or commit before applying the same pinned revision:

```bash
ctx update --ref main
ctx update --ref main --yes
```

The updater builds a native package, preserves configuration and data, restarts
only previously active CortexFS units, and restores the exact prior package if
health verification fails. After the first successful ref update, plain
`ctx update` reuses the recorded tracking ref.

Run `ctx update` as a normal user with `sudo`, a controlling terminal, and
`flock`. Do not run it as root. The first plan requires `--ref` or `--source`;
`--yes` applies that pinned commit. Constraints and packaged binaries are in
the [Linux packages guide](packaging.md).

## Start A Session

The short interactive commands use the current folder as the workspace:

```bash
ctx                 # create a fresh session here and open chat
ctx resume          # resume this folder's session
ctx status          # show CortexFS status
ctx help            # show concise CLI help
```

`ctx resume executor --session SESSION` restores an explicit session. The default
interactive agent is `executor`; set `CTX_DEFAULT_AGENT` to choose another agent.
The longer `ctx agent ...` commands remain available for lifecycle, raw socket,
PTY, history, and diagnostic operations.

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
ctx file type tool/fs.read
ctx file tool/fs.read
```

`model/debug/echo` is the smallest debug model. It echoes input and is useful
for confirming that local installation and ABI paths work.

## Start An Agent Terminal

`ctx agent start` uses `systemd-run --user` to start `ctxterm -> tsh` inside a
bwrap sandbox. By default, it mounts the caller's current directory read-write
at `/workspace`. If the current directory contains `.git`, it is additionally
over-mounted read-only at `/workspace/.git`. The agent starts with `pwd` set to
`/workspace`, while `HOME` is the sandbox's own `/home/agent`:

```bash
ctx agent start executor --session default
ctx agent watch executor --session default
ctx agent attach executor --session default
```

`watch` only observes terminal output; `attach` joins the terminal and writes
stdin. Declare additional mounts explicitly:

```bash
ctx agent start executor --session docs \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

`tsh` is not a host shell. It only looks up CortexFS tools through `CTX_PATH`,
such as `/ctx/tool` and `/ctx/home/<uid>/tool`. `bash`, `tmux`, and `zellij`
must also be visible as tools before they can run.

## Troubleshoot The Install

`ctx doctor` prints one status line per ABI check and exits non-zero when any
check fails. Typical lines:

```text
ok root /ctx
ok status
ok agent
ok agent/executor
ok bootstrap tree_version=9 migrations=retired-agents,rolling-tree,agent-update,current-models,agent-permissions,initial-agents
stale agent/coder (retired reference agent; run ctx bootstrap --check)
missing bootstrap state (run ctx bootstrap)
```

| Symptom | What to do |
| --- | --- |
| `missing root /ctx` or `ctx status` fails | Start the mount: `sudo systemctl start cortexfs.service`. Native packages enable the unit but do not start it on first install. |
| FUSE / `fusermount3` / `/dev/fuse` errors | Load the kernel module (`sudo modprobe fuse`) and install `fuse3` plus `pkg-config fuse3`. Containers need the FUSE device. |
| bubblewrap too old | The installer and updater require `/usr/bin/bwrap` 0.10+. Upgrade through the distribution; CortexFS does not overwrite host `bwrap`. |
| `stale` / `stale-user` `agent/coder` or `agent/worker` | Those names are retired leftovers. The managed tree is `architect`, `executor`, and `product-manager`. `agent/main` aliases `executor`. Run `ctx bootstrap --check`, then `ctx bootstrap` if you want the current tree. Do not set `agent/coder.d/model`. |
| An older installer stops at `agent/coder.d/model` | Older revisions targeted retired `coder` during optional onboarding and could stop before verification on a clean install. The current installer uses `executor`. Rerun the current installer, bind the configured model with `ctx set agent/executor.d/model PROVIDER/MODEL`, and run `ctx doctor`. Existing channels should point at `/ctx/agent/main.sock` or `/ctx/agent/executor.sock`. |
| `/ctx` is missing after `ctx update` | The updater restarts only units that were active, including the exact `cortexfs.service` name. If the mount was inactive before the transaction, start it: `sudo systemctl start cortexfs.service`. |
| `ctx agent start` cannot talk to user systemd | The terminal path uses `systemd-run --user`. Log into a user session that has a user bus; lingering is a host policy, not a CortexFS ABI. |
| `ctx update` refuses to run | Use a non-root login with `sudo`, `/dev/tty`, and `flock`. The installed helper is `/usr/lib/cortexfs/update-linux` and must stay root-owned. |
| Interactive agent hits memory/CPU/task ceilings | Terminals use `MemoryMax=1G` / `CPUQuota=200%` / `TasksMax=256`. Socket-activated runtimes use `512M` / `100%` / `128`. See [Agent Runtime](spec/agent-runtime.md). |
| Default chat talks to the wrong agent | The default is `executor`. Override with `CTX_DEFAULT_AGENT`. |

`ctx doctor` checks ABI shape, not provider credentials. An `unconfigured`
model still needs `ctx provider preset` and `ctx auth login`.

## Next Step

Continue with [Daily Usage](using-cortexfs.md): `ctx`, `agent.sh`, shared
directories, and session history.
