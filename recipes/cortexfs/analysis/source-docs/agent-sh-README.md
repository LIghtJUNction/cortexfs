# agent.sh

`agent.sh` is a small defaults wrapper over `ctx agent ...`. It is not a
runtime, socket protocol implementation, provider SDK, policy engine,
scheduler, or chat database.

It depends on Bash and one `ctx` binary. All agent protocol behavior, line
editing, response rendering, terminal attach, and socket IO stays inside `ctx`.

The stable paths below are the CortexFS state that `ctx` reads and writes:

```text
/ctx/agent/<agent>.sock
/ctx/agent/<agent>.d/
/ctx/home/<uid>/agent/<agent>/session/
/ctx/tool
/ctx/home/<uid>/tool
/ctx/shared
```

It does not use root namespaces such as `provider`, `format`, `cluster`,
`control`, `thread`, `workflow`, `mcp`, or `skill`.

## Usage

```bash
export CTX_ROOT=/ctx
export CTX_HOME="/ctx/home/$(id -u)"
export CTX_PATH="$CTX_ROOT/tool:$CTX_HOME/tool"

./agent.sh coder
./agent.sh coder "fix tests"
./agent.sh --chat coder
./agent.sh --repl coder
./agent.sh --attach coder
./agent.sh --watch coder
./agent.sh --session default coder
./agent.sh --resume coder
./agent.sh --history coder
./agent.sh --pack coder
./agent.sh --tools coder
./agent.sh --children coder
./agent.sh --cancel coder
```

With no prompt, `agent.sh AGENT` opens the agent chat UI through
`ctx agent chat AGENT`. With a prompt, it dispatches to
`ctx agent send AGENT`. Use `agent.sh --watch AGENT` to observe the agent
terminal read-only, or `agent.sh --attach AGENT` when you want to join the
terminal and see `ctxterm -> tsh`.

When run from a CortexFS checkout, `agent.sh` prefers `../target/debug/ctx`
or `../target/release/ctx` before the `ctx` found on `PATH`. Set `CTX_BIN` to
force an exact ctx binary:

```bash
CTX_BIN=/path/to/ctx ./agent.sh coder
```

The socket request shape used by `ctx` is JSONL:

```jsonl
{"op":"send","id":"ctx-...","session":"default","scope":"private","cwd":"/workspace","input":"fix tests"}
{"op":"resume","session":"default"}
{"op":"cancel","id":"run-1"}
```

Responses are rendered by `ctx agent` as assistant text by default. Interactive
REPL responses are buffered before printing so model output does not corrupt the
user's current input buffer. Pass `--raw` to print raw JSONL.

## Sessions

`agent.sh` never stores private history. It reads the current rolling session tree:

```text
$CTX_HOME/agent/<agent>/session/index/current
$CTX_HOME/agent/<agent>/session/<session>/messages.jsonl
$CTX_HOME/agent/<agent>/session/<session>/events.jsonl
$CTX_HOME/agent/<agent>/session/<session>/latest.md
$CTX_HOME/agent/<agent>/session/<session>/context/
```

If no session is selected, `index/current` is used when present, otherwise
`default`.

Use `ctx agent output <agent>` to print the latest assistant output for the
selected session.

## Tools And Children

`--tools` lists executable files found through `CTX_PATH` and
`agent/<agent>.d/path`. It does not decide policy locally.

`--children` reads:

```text
$CTX_HOME/agent/<agent>/session/<session>/context/child/
```

The script is dependency-free in the project sense: no `nc`, `jq`, Python,
Node, npm, Cargo, cloud SDK, provider client, package manager, or direct
provider API.
