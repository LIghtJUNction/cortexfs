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
Do that by changing the alias instead of adding provider-specific root entries.
Provider secrets are not written into model files or `.d/` control
directories; provider adapters resolve API keys from environment variables,
the system keychain, then the unconfigured state.

Install file-based presets for common providers first:

```bash
ctx provider preset list
ctx provider preset show google
ctx provider preset install codex
ctx provider preset install openai
ctx provider preset install anthropic
ctx provider preset install google
```

Canonical provider names are `openai`, `anthropic`, and `google`. `codex` is
an alias for the `openai` preset; `gemini` is an alias for the `google` preset.
After installing `codex`, models are still projected under the canonical
`/ctx/model/openai/<model>` path. CortexFS does not add a
`/ctx/model/codex` namespace.

Model proxying is not an agent and is not written into provider JSON. The
single global route table is:

```text
/ctx/model/route
```

This file decides both transport and key slot. Multiple providers, multiple
models, and multiple keys for one provider all route through this table:

```text
group(proxy) -> http(http://127.0.0.1:8080/v1), key(office)
group(local-socket) -> unix(/run/user/1000/cortexfs/proxy/openai.sock), key(local)

dip(198.51.100.45) -> direct
# dip(203.0.113.43) -> JP
domain(bestproxy.com) -> proxy
pname(NetworkManager, systemd-resolved, dnsmasq) -> must_direct
dip(geoip:private) -> direct
dip(geoip:cn) -> direct
domain(geosite:cn) -> direct
model(embedding-*) -> local-socket
fallback: proxy
```

`key(office)` means another credential slot for the same provider. CortexFS
first checks the matching environment variable, then the system keychain entry
`service=cortexfs:<provider> account=office`. Without `key(...)`, it uses
`account=default`.

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
the sandbox by default, then starts `ctxterm -> tsh` from `/workspace`. If the
caller directory contains `.git`, `.git` is additionally over-mounted read-only
at `/workspace/.git`. The agent's `HOME` is the sandbox's own `/home/agent`, so
shell configuration and caches are not written into the project directory:

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
`/run/user/<uid>/cortexfs/terminal/.../main.sock`; older installs may also
point to `/run/cortexfs/terminal/.../main.sock`. `watch` is read-only; `attach`
connects your stdin to the terminal.

Control the sandbox explicitly when needed:

```bash
ctx agent start coder --session review \
  --no-default-workspace \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

## Use The Tool Shell

`tsh` is the CortexFS tool shell, not a host shell. Standalone human `tsh`
resolves commands in this order:

```text
1. CTX_HOME/.tshrc line CTX_PATH=...
2. process CTX_PATH
3. default /ctx/tool:/ctx/home/<uid>/tool
```

Inside an agent terminal, `tsh` uses the process `CTX_PATH` that the agent
runtime derives from policy, mounts, and uid/gid. User `.tshrc` does not
override that authorization path.

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

When invoking tools directly, prefer doing it from the agent terminal through
`tsh`, so CortexFS can apply agent policy, mounts, uid/gid, and `CTX_PATH`
together.

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

## Customize Agents

User-editable system prompts live at:

```text
/ctx/agent/<agent>.d/system.md
/ctx/agent/<agent>.d/prompt.template.md
```

For example:

```bash
ctx cat agent/coder.d/system.md
ctx set agent/coder.d/system.md "You are a careful Rust coding agent."
ctx cat agent/coder.d/prompt.template.md
ctx agent prompt coder
```

`system.md` only defines persona and working style. `prompt.template.md`
defines how that content is combined with rules, skill metadata, tool
injection, history context, and the runtime contract into the first system
message visible to the model. Template variables include `{{agent}}`,
`{{current_time_unix}}`, `{{agent_instructions}}`, `{{rules}}`, `{{skills}}`,
`{{tool_injection}}`, `{{history_messages}}`, and `{{runtime_contract}}`.

`ctx agent prompt <agent>` prints the runtime system prompt that CortexFS can
currently render. Use it to inspect the template, agent instructions,
discoverable AGENTS.md rules, bounded skill metadata, and runtime contract. At
real model-call time, tool injection and history context are still filled by
the runtime according to the context window.

The skill list only injects `name`, `description`, and the `SKILL.md` path.
Full `SKILL.md` content is read only after a skill is selected. Skill metadata
may use at most 2% of the context window; when the window size is unknown, the
hard cap is 8,000 characters. Over budget, descriptions are shortened first;
if still over budget, some skills are omitted and the prompt includes a
warning.

These prompt files do not grant authority. The default native tool remains
`tsh`; other tools must be discovered, loaded, pinned, and called through
`tsh`. Effective authority is still decided by `agent/<agent>.d/policy`,
`path`, `mount`, Linux uid/gid, and mode bits.

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
back to `default`. That means inspecting the current/latest session does not
need a separate `latest` subcommand.

The underlying history lives at:

```text
/ctx/home/<uid>/agent/<agent>/session/
```

Raw history is durable fact; context is a rebuildable working set. Compacting
context must not destroy raw messages.
