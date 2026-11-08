---
id: getting-started
title: Start With Installation
sidebar_label: Start With Installation
---

> Historical guide · documentation through [`7261cb3`](https://github.com/LIghtJUNction/cortexfs/commit/7261cb399ba1f8efa387736938ea3d8eec1a0acb).
> Product commands require the corresponding historical source or installed build;
> this checkout contains documentation and recipes.
> [Documentation index](README.md) · [Reconstruction task](../recipes/cortexfs/BUILD_PROMPT.md)

# Start With Installation

This guide records the historical installation and first checks. For the
current repository, start with the [reconstruction recipe](../recipes/cortexfs/RECIPE.md).

## Install

The source installer targets systemd-based Arch, Debian/Ubuntu, Fedora/RHEL,
and openSUSE/SLES systems with the required distribution packages. It checks
FUSE, bubblewrap 0.10+, and Rust 1.91+ before building with `Cargo.lock`.
Pin both the installer and its source download to the documentation snapshot:

```bash
cortexfs_ref=7261cb399ba1f8efa387736938ea3d8eec1a0acb
curl -fsSL "https://raw.githubusercontent.com/LIghtJUNction/cortexfs/$cortexfs_ref/scripts/install.sh" \
  | CORTEXFS_REF="$cortexfs_ref" sh
```

The installer requests typed confirmation for persistent changes. Re-running
updates programs and units while preserving storage, secrets, provider
configuration, environment files, and `/ctx` user state. First-install
onboarding can configure a provider through the existing provider commands.

Use `ctx doctor` to inspect the mount, base directories, default model alias,
and runtime state. `ctx --help` lists the installed build's commands.
Native packages are covered in [Linux Packages](packaging.md); IM setup is in
[Multi-IM channels](channels.md).

## Update The Host

Inspect a plan, then apply the same historical source revision:

```bash
cortexfs_ref=7261cb399ba1f8efa387736938ea3d8eec1a0acb
ctx update --ref "$cortexfs_ref"
ctx update --ref "$cortexfs_ref" --yes
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

Inspect the mount with `ctx status` and `ctx ls`. The
[root ABI](spec/root-abi.md) has exactly eight entries:

```text
status
bin/
model/
agent/
tool/
channel/
home/
shared/
```

Provider configuration, workflows, databases, MCP registries, and skills stay
outside the root namespace.

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
at `/workspace`. Ordinary Git metadata stays writable with that workspace;
CortexFS does not implicitly over-mount `.git` read-only. Explicit path policy
may still make `/workspace/.git` read-only, and an out-of-tree linked-worktree
gitdir still needs its own mount. The agent starts with `pwd` set to
`/workspace`, while `HOME` is the sandbox's own `/home/agent`. Pass `-- COMMAND`
to replace `tsh` with a hosted Agent CLI:

```bash
ctx agent start executor --session default
ctx agent start executor --session default -- /usr/bin/codex
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

`ctx doctor` reports each ABI check and exits non-zero on failure. Use the
reported path or component to choose the next check:

| Symptom | What to do |
| --- | --- |
| `missing root /ctx` or `ctx status` fails | Start the mount: `sudo systemctl start cortexfs.service`. Native packages enable the unit but do not start it on first install. |
| FUSE / `fusermount3` / `/dev/fuse` errors | Load the kernel module (`sudo modprobe fuse`) and install `fuse3` plus `pkg-config fuse3`. Containers need the FUSE device. |
| bubblewrap too old | The installer and updater require `/usr/bin/bwrap` 0.10+. Upgrade through the distribution; CortexFS does not overwrite host `bwrap`. |
| Hosted CLI has no network | Default sandbox is `--unshare-net`. Explicit `ctx agent start NAME -- COMMAND` enables pasta egress only when policy allows `network:default connect`, `/usr/bin/pasta` exists, and `/dev/net/tun` exists. Arch packages `passt`; Debian/RPM metadata currently omit it. Missing pasta or tun stays isolated. |
| Hosted CLI cannot write `/ctx` | Writable `/ctx` is only for explicit hosted commands using the canonical `/ctx` root after `findmnt` confirms a read-write `fuse`/`fuse.cortexfs` CortexFS mount. Legacy no-command `tsh` launches stay read-only. |
| `stale` / `stale-user` `agent/coder` or `agent/worker` | Those names are retired leftovers. The managed tree is `architect`, `executor`, and `product-manager`. `agent/main` aliases `executor`. Run `ctx bootstrap --check`, then `ctx bootstrap` if you want the current tree. Do not set `agent/coder.d/model`. |
| An older installer stops at `agent/coder.d/model` | Older revisions targeted retired `coder` during optional onboarding and could stop before verification on a clean install. The pinned installer uses `executor`. Rerun that installer, bind the configured model with `ctx set agent/executor.d/model PROVIDER/MODEL`, and run `ctx doctor`. Existing channels should point at `/ctx/agent/main.sock` or `/ctx/agent/executor.sock`. |
| `/ctx` is missing after `ctx update` | The updater restarts only units that were active, including the exact `cortexfs.service` name. If the mount was inactive before the transaction, start it: `sudo systemctl start cortexfs.service`. |
| `ctx agent start` cannot talk to user systemd | The terminal path uses `systemd-run --user`. Log into a user session that has a user bus; lingering is a host policy, not a CortexFS ABI. |
| `ctx update` refuses to run | Use a non-root login with `sudo`, `/dev/tty`, and `flock`. The installed helper is `/usr/lib/cortexfs/update-linux` and must stay root-owned. |
| Interactive agent hits memory/CPU/task ceilings | Terminals use `MemoryMax=1G` / `CPUQuota=200%` / `TasksMax=256`. Socket-activated runtimes use `512M` / `100%` / `128`. See [Agent Runtime](spec/agent-runtime.md). |
| Default chat talks to the wrong agent | The default is `executor`. Override with `CTX_DEFAULT_AGENT`. |

`ctx doctor` checks ABI shape, not provider credentials. An `unconfigured`
model still needs `ctx provider preset` and `ctx auth login`.

## Next Step

Continue with [Daily Usage](using-cortexfs.md) for `ctx`, `agent.sh`, shared
directories, and session history, or return to the [documentation index](README.md).
