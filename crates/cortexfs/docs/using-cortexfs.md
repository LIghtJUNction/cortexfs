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

On some deployments, `/ctx/agent/<name>.sock` is an owner-authorized symlink
to a user runtime socket; on some system deployments it may also be a direct
socket node. Treat both as valid implementation forms and probe with `nc -U` or
`readlink` according to what the mount currently exposes.

## Call A Model Directly

Start with the echo model while debugging:

```bash
/ctx/model/debug/echo "hello cortex"
echo "summarize this file" | /ctx/model/main
```

Change the `/ctx/model/main` alias when you want a different default model.
Do that by changing the alias instead of adding provider-specific root entries.
The reference tree provides `architect`, `coder`, `reviewer`, and `worker`.
`architect` is the root planning and coordination agent; `coder`, `reviewer`,
and `worker` use `agent:architect` as their parent.

Bootstrap and inspect the reference source with:

```bash
ctx bootstrap
ctx bootstrap --check
ctx bootstrap --dry-run
```

`ctx bootstrap` writes `bin/cortexfs.bootstrap.json` only when the schema,
tree version, managed-agent list, or required migrations need refresh. Retired
`base` and `executor` objects are reported but retained for
manual review because old installations have no manifest proving ownership and
full control-tree integrity. A successful bootstrap makes the next `--check`
clean.

The default `coder.d/system.md` treats `coder` as the parent integrator:
independent implementation work should be a delegated `react` node in
`context/plan.json`, delegated nodes that omit `agent` use `worker`, and
delegated nodes that omit `session` use the current parent session name.
Advance one schedule step with:

```bash
ctx schedule status home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule advance home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule claim home/1000/agent/coder/session/default/context/plan.json work-123
ctx schedule result home/1000/agent/coder/session/default/context/plan.json work-123 done "implemented"
ctx agent wait coder work-123 --session default
```

`status` reads the plan, child table, and delegated worker
`agent/<name>`, `agent/<name>.d/model`, and `life`; it does not invent
`main`/`owned` defaults when the delegated backing agent is missing, then prints
`node<TAB>kind<TAB>agent<TAB>child<TAB>session<TAB>model<TAB>life<TAB>state`. `advance`
materializes ready child handoffs, `claim` moves a materialized child from
`pending` to `active`, and `result` writes the terminal child result under
`context/child/<child>/`. Command output includes the parent ref plus the child
`agent`, `session`, `model`, `life`, `handoff.md`, `result.md`, and
`refs.jsonl` ABI paths so neither parent nor worker has to guess coordination
state. `agent wait` is a non-blocking waitpid-shaped reader: while the child is
`pending` or `active` it fails, and once the child is `done`, `error`, or
`cancelled` it prints
`child<TAB>status<TAB>agent<TAB>session<TAB>model<TAB>life` followed by
`result.md`. These commands do not start background listeners, polling loops,
or a second submission entrance.
Provider secrets are not written into model files or `.d/` control
directories; provider adapters resolve API keys from provider environment
candidates first (if set), then the CortexFS system secret store
(`/var/lib/cortexfs/secrets/provider/<provider>/<slot>`). If a required
credential is absent, the model is considered `unconfigured`.

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

`key(office)` means another credential slot for the same provider and
selects `/var/lib/cortexfs/secrets/provider/<provider>/office`.
Without `key(...)`, CortexFS uses the `default` slot.

## Manage Agents

`ctx agent` is the thin client for the current ABI. Creating, starting, and
stopping agents still goes through ordinary tools or file ABI; it does not add
a workflow entrance:

```bash
ctx agent new reviewer --model openai/gpt-5.6 --tool fs.read
ctx agent new reviewer --label reviewer_t --shared project-a:read --mount /work /work ro
ctx agent new --from .cortexfs/agents/reviewer/agent.yaml
ctx agent apply reviewer --from reviewer
ctx agent start reviewer --session default
ctx agent status reviewer
ctx agent ps
ctx agent stop reviewer
```

Host-side `agent.yaml` files (also `agent.yml` and `agent.json`) are
authoring inputs. `ctx agent new --from` and `ctx agent apply --from`
validate and materialize them into `agent/<name>.d/*`; runtime authority
continues to come only from the discrete control files. A short `--from NAME`
searches `.cortexfs/agents` and `~/.config/cortexfs/agents`.

```yaml
schema: cortexfs.agent.profile/v1
name: reviewer
description: code review agent
instructions: Review diffs carefully.
model: openai/gpt-5.6
tools: [fs.read]
parent: agent:architect
```

`ctx agent new` prefers `/ctx/tool/agent.create`; if that tool is absent, host
`ctx` creates the standard `agent/<name>.d/*` control files and
`home/<uid>/agent/<name>/` skeleton. `ctx agent start` starts the explicit
runtime; once the terminal socket is reachable it writes
`agent/<name>.d/status=ready` and appends an `agent.start` event to
`agent/<name>.d/log`. `ctx agent stop` prefers `/ctx/tool/agent.stop`; if that
tool is absent it writes `agent/<name>.d/status=dead`, clears `pid`, and
appends an `agent.stop` event. `ctx agent status` and `ctx agent ps` only read
ordinary `agent/<name>.d/*` control files. `agent status` keeps the first line
as the status value, then prints `model=...`, `life=...`, `parent=...`,
`children=...`, `pid=...`, `uid=...`, `gid=...`, `groups=...`, `root=...`, and
`cwd=...`. `children=...` counts direct children whose effective state is not
`dead`; `ready` or `busy` children with stale numeric pids are excluded the
same way as `ctx agent ps`.
Non-default models and non-`owned` lifecycles are visible in `ctx agent ps`.
`ctx agent env NAME` prints the sandbox environment derived by
`ctx agent start`, and `ctx agent children NAME` shows parent-side child state
plus the backing worker `parent_session`, `model`, `life`, `status`, and `pid`.

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

The FUSE-visible path aliases the root-owned
`/run/cortexfs/terminal/broker.sock`. `ctx` authenticates the broker and asks
for the named agent/session; it does not connect to legacy per-user terminal
sockets. `watch` is read-only; `attach` connects your stdin to the terminal.

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

Agents with the `agent.update` grant can iterate themselves: the tool
atomically replaces the calling agent's own `system.md` or
`prompt.template.md` through the host-validated run capability socket, and the
new prompt applies from the next run. Other agent controls stay host-owned;
see `docs/spec/tool-policy-abi.md` for the exact contract.

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

`agent.sh coder` opens the chat UI through
`ctx agent chat coder --session default`. With prompt arguments, it forwards one
message to `ctx agent send coder --session default`. Use
`agent.sh --watch coder` to observe the agent terminal, and `agent.sh --attach
coder` only when you want to enter `ctxterm -> tsh`. `agent.sh` does not keep a
private chat database.

## Installed Multi-Turn Smoke

After installation, the minimal multi-turn smoke should use the existing
session ABI instead of adding a test entrance:

```bash
ctx bootstrap
ctx agent start coder --session default --cwd /workspace
ctx agent send coder --session default "round one: read the current task"
ctx agent send coder --session default "round two: continue from the previous turn"
ctx agent history coder --session default
ctx agent output coder --session default
```

This path checks `agent/<agent>.sock`, `messages.jsonl`, `latest.md`, current
session selection, and prompt-history injection. Durable conversation facts
stay in `/ctx/home/<uid>/agent/<agent>/session/<session>/messages.jsonl`;
`ctx agent prompt` is only for inspecting the prompt that would be sent to the
model, not a substitute for a live socket conversation.

When handing independent implementation work to the spark worker, the parent
first materializes the handoff with `ctx schedule advance`, then gives the
worker the emitted `model=`, `life=`, `plan=`, `handoff=`, `result=`, and
`refs=` fields. The worker writes back through the same
`ctx schedule claim/result` path; do not add a queue, poller, or second
coordination file.

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

When an agent run builds its prompt, CortexFS writes a best-effort load
snapshot into that agent's private session directory (same text as
`{{rules}}` / `{{skills}}`; snapshot write never blocks the run):

```bash
cat /ctx/home/$(id -u)/agent/coder/session/default/AGENTS.md
cat /ctx/home/$(id -u)/agent/coder/session/default/SKILLS.md
```

- `AGENTS.md`: effective merged rules (global + project layers)
- `SKILLS.md`: skill metadata only (`name` / `description` / `path`), not full
  `SKILL.md` bodies

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
ctx agent trajectory coder
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
`ctx agent trajectory` validates and prints an ATIF JSON projection of the
selected session's `messages.jsonl` and `events.jsonl`. Tool calls,
observations, and usage remain associated by run and call id; the command does
not create a second history store.
