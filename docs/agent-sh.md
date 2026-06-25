# agent.sh

`agent.sh` is a tiny Linux shell frontend for the CortexFS v1 agent ABI. It is
not the CortexFS runtime, a provider SDK, a scheduler, or a private chat
database.

It only depends on ordinary shell tools plus Linux `nc` with Unix socket
support. It does not use `jq`, Python, Node, npm, Cargo, cloud SDKs, provider
clients, or package managers.

## Install

Install the repository copy somewhere on `PATH`:

```bash
install -m 0755 agent.sh/agent.sh ~/.local/bin/agent.sh
```

Check the installed frontend:

```bash
agent.sh --help
```

## ABI Paths

`agent.sh` only uses stable v1 paths:

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

With no prompt, `agent.sh AGENT` reads lines from stdin or an interactive TTY
from an interactive TTY attaches to the agent terminal through
`ctx agent attach AGENT`, so the user sees `ctxterm -> tsh`. With a prompt, it
delegates to `ctx agent send AGENT`.

Use `agent.sh --chat AGENT` for the agent socket chat REPL.

## Socket Protocol

Socket requests are newline-delimited JSON:

```json
{"op":"send","id":"agent-sh-...","session":"default","scope":"private","cwd":"/work","input":"fix tests"}
{"op":"resume","session":"default"}
{"op":"cancel","id":"run-1"}
```

Responses are rendered as assistant text by default. Set
`CORTEX_RAW_EVENTS=1` or pass `--raw` to print raw JSONL events.

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
