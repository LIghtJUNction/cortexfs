# agent.sh

`agent.sh` is a tiny Linux defaults frontend for the Rust-owned
`ctx agent` commands. It is not the CortexFS runtime, a socket protocol
implementation, a provider SDK, a scheduler, or a private chat database.

It depends on Bash and one `ctx` binary. It does not use `nc`, `jq`, Python,
Node, npm, Cargo, cloud SDKs, provider clients, package managers, or direct
provider APIs. All agent protocol behavior stays inside `ctx`.

## Install

Install the repository copy somewhere on `PATH`:

```bash
install -m 0755 agent.sh/agent.sh ~/.local/bin/agent.sh
```

Check the installed frontend:

```bash
agent.sh --help
```

## Boundary

`agent.sh` is a small defaults wrapper, not an ABI reader. It resolves `ctx` and
then execs `ctx agent ...` with common defaults such as `--session default`.
The stable paths below are the CortexFS state that `ctx` reads and writes:

```text
/ctx/agent/<agent>.sock
/ctx/agent/<agent>.d/
/ctx/home/<uid>/agent/<agent>/session/
/ctx/tool
/ctx/home/<uid>/tool
/ctx/shared
```

`/ctx/tool` is the system tool tier. `/ctx/home/<uid>/tool` is the user's own
tool tier, not a place for default symlink copies of system tools. An actual
agent runtime may see a filtered in-memory FUSE projection of these tiers.

It does not use root namespaces such as `provider`, `format`, `cluster`,
`control`, `thread`, `workflow`, `mcp`, or `skill`.

## Environment

```bash
export CTX_ROOT=/ctx
export CTX_HOME="$CTX_ROOT/home/$(id -u)"
export CTX_PATH="$CTX_ROOT/tool:$CTX_HOME/tool"
```

Defaults are derived from the same values when these variables are not set.
`CTX_PATH` is a list of source tiers; policy, mounts, uid/gid, and mode bits
still decide what a specific agent may execute.

## Usage

```bash
agent.sh coder
agent.sh coder "fix tests"
agent.sh --chat coder
agent.sh --attach coder
agent.sh --watch coder
agent.sh --session default coder
agent.sh --resume coder
agent.sh --history coder
agent.sh --pack coder
agent.sh --tools coder
agent.sh --children coder
agent.sh --cancel coder
agent.sh --status coder
agent.sh --raw coder "prompt"
```

With no prompt, `agent.sh AGENT` opens the agent chat REPL through
`ctx agent chat AGENT --session default`. With a prompt, it dispatches to
`ctx agent send AGENT --session default`.

Use `agent.sh --watch AGENT` to observe the agent terminal read-only. Use
`agent.sh --attach AGENT` only when you want to join the terminal and see
`ctxterm -> tsh`.

## Chat And Terminal

`ctx agent chat` and `ctx agent repl` own line editing, interrupt handling, socket requests, and
assistant response rendering. Interactive REPL responses are buffered before
printing so model output does not corrupt the user's current input buffer.
`Ctrl+C` exits an idle REPL. While a run is active it asks CortexFS to cancel
that run and returns to the prompt.

`ctx agent send` is the non-interactive path and may stream assistant deltas as
they arrive.

`ctx agent attach` is a different workflow: it joins the persistent agent PTY.
That PTY runs `ctxterm -> tsh`; `tsh` is the agent-facing tool shell, not the
human chat UI.

The socket request shape used by `ctx` is newline-delimited JSON:

```json
{"op":"send","id":"ctx-...","session":"default","scope":"private","cwd":"/workspace","input":"fix tests"}
{"op":"resume","session":"default"}
{"op":"cancel","id":"run-1"}
```

Responses are rendered by `ctx agent` as assistant text by default. Pass `--raw`
to print raw JSONL events.

## Sessions

`agent.sh` never stores private history. It reads the v1 session tree:

```text
$CTX_HOME/agent/<agent>/session/index/current
$CTX_HOME/agent/<agent>/session/<session>/messages.jsonl
$CTX_HOME/agent/<agent>/session/<session>/events.jsonl
$CTX_HOME/agent/<agent>/session/<session>/latest.md
$CTX_HOME/agent/<agent>/session/<session>/context/
```

If no session is selected, `index/current` is used when present, otherwise the
session name is `default`.

Use `ctx agent output <agent>` to print the latest assistant output for the
selected session. Omitting `--session` follows the same `index/current`, then
`default` rule.

## Tools And Children

`--tools` lists executable files found through `CTX_PATH` and
`agent/<agent>.d/path`. It does not decide policy locally.

`--children` reads child task state from:

```text
$CTX_HOME/agent/<agent>/session/<session>/context/child/
```
