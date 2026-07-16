# Agent Runtime

This document defines the v1 end-to-end agent runtime shape. It ties together
the agent object ABI, the human CLI, socket sessions, the persistent terminal,
the `tsh` tool shell, prompt construction, and sandbox execution.

It does not add a root namespace. Everything here is derived from existing v1
objects and files.

## Runtime Surfaces

There are three separate surfaces:

```text
human chat       ctx agent chat/send/resume/cancel
human terminal   ctx agent watch/attach
agent tool use   tsh inside ctxterm
```

They must not collapse into one interface.

`ctx agent chat` is a human chat UI. It owns line editing, `Ctrl+C`, socket
requests, assistant response rendering, and prompt re-display. Interactive chat
responses are buffered before printing so assistant output cannot corrupt the
user's input buffer. `Ctrl+C` exits an idle chat; while a run is active it first
requests cancellation for that run and returns to the prompt.

`ctx agent send` is a non-interactive human command. It may stream assistant
deltas as they arrive.

`ctx agent attach` and `ctx agent watch` join the persistent agent terminal.
That terminal is not the human chat UI. It exists so humans can observe or join
the same PTY used by the agent-facing shell.

`tsh` is the agent-facing tool shell. It is a tool and a standalone binary, but
it is not a host shell. It resolves commands through `CTX_PATH`, never through
host `PATH`.

## Default Human Entry

`agent.sh` is a small defaults frontend over the Rust-owned `ctx agent`
commands:

```text
agent.sh AGENT           -> ctx agent chat AGENT --session default
agent.sh AGENT INPUT...  -> ctx agent send AGENT --session default INPUT...
agent.sh --watch AGENT   -> ctx agent watch AGENT --session default
agent.sh --attach AGENT  -> ctx agent attach AGENT --session default
```

`agent.sh` must remain a tiny defaults wrapper. Socket
protocols, terminal emulation, model streaming, policy checks, tool discovery,
and provider behavior belong in CortexFS, primarily under `ctx`.

## Socket Chat Flow

The stable request flow is:

```text
human
  -> ctx agent chat/send
  -> agent/<name>.sock
  -> socket runtime
  -> durable session files
  -> selected model/agent executable
  -> event JSONL
  -> ctx renderer
```

Socket requests are JSONL. They name a session and operation:

```json
{"op":"send","id":"msg-1","session":"default","scope":"private","cwd":"/workspace","input":"hello"}
{"op":"resume","session":"default"}
{"op":"cancel","id":"run-1"}
```

The socket runtime records user messages before invoking the agent/model path.
Assistant text is derived from stable event frames and recorded back to the
durable session. Raw messages and events remain ordinary files; context packs
are rebuildable views.

Before invoking an executable agent, the socket runtime writes exactly one
`sdk-envelope-v1` frame to stdin. Its `history_messages` and `tool_context`
fields carry bounded prompt context; the agent boundary does not expose legacy
`CTX_AGENT_HISTORY_MESSAGES` or `CTX_AGENT_TOOL_CONTEXT` environment inputs.

If a human sends `SIGINT` while a run is active, `ctx agent chat` sends a
`cancel` request for the active run id and returns to the prompt. In an idle
interactive chat, `Ctrl+C` exits the chat.

The socket-activated executable agent runtime observes the durable session state
for the active run. When the matching `done/cancelled` event appears, it stops
the executable agent process group with `SIGTERM`, escalates to `SIGKILL` after
a short grace period, and does not record later assistant output for that run.

## Persistent Terminal Flow

The terminal flow is:

```text
ctx agent start
  -> systemd-run --user
  -> bwrap sandbox
  -> ctxterm --listen SOCKET -- /ctx/bin/tsh
  -> tsh
```

`ctxterm` owns the pseudo-terminal. It exposes `watch` and `attach` modes
through the session terminal socket.

Session terminal sockets are visible through the ABI path:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

User-started terminals may place the real socket under:

```text
/run/user/<uid>/cortexfs/terminal/<agent>/<session>/main.sock
```

`ctx agent attach` should try the ABI path first, then the user runtime path.
If both locations are unavailable, it returns a socket-availability error.

## Sandbox Contract

`ctx agent start` creates the default interactive terminal sandbox. Unless
overridden, it binds the caller's current working directory at `/workspace` with
read-write access and starts the terminal there. If the host directory contains
`.git`, `.git` is over-mounted at `/workspace/.git` read-only.

The sandbox home is:

```text
HOME=/home/agent
```

It is backed by:

```text
/ctx/home/<uid>/agent/<agent>
```

Shell state such as `.config`, `.cache`, and `.bash_history` must land in the
agent home, not in the project workspace.

The terminal process starts from an empty environment. CortexFS injects only a
small allowlist through the sandbox launcher:

```text
CTX_ROOT
CTX_HOME
CTX_AGENT
CTX_AGENT_SUBJECT
CTX_PATH
HOME=/home/agent
USER
LOGNAME
SHELL
TERM
LANG
```

Host session variables and provider secrets must not be inherited by default.
Executable agents launched from the socket runtime also start with
`env_clear()` and receive only the derived agent environment plus runtime-owned
`CTX_*` values.

## Agent View And Authority

An agent runtime view is derived from:

```text
agent/<name>.d/root
agent/<name>.d/cwd
agent/<name>.d/env
agent/<name>.d/path
agent/<name>.d/mount
agent/<name>.d/model
agent/<name>.d/policy
agent/<name>.d/uid
agent/<name>.d/gid
agent/<name>.d/groups
agent/<name>.d/label
```

Prompt text, `AGENTS.md`, skill metadata, `.mcp.json`, and tool descriptions
may influence model behavior, but they do not grant authority.

Actual authority is the intersection of:

```text
mount/chroot visibility
Linux uid/gid/groups and mode bits
CortexFS label and policy
CTX_PATH tool visibility
tool executable metadata and noexec placement
```

Both the filesystem layer and the CortexFS policy layer must allow an action.

## Tool Shell Contract

Agents should see one native callable tool by default:

```text
tsh
```

Other tools are discovered, loaded, pinned, and invoked through `tsh`.

`tsh` resolves tools by `CTX_PATH`. For standalone human sessions, it reads the
data-only startup file before inherited process `CTX_PATH` when the file exists:

```text
CTX_HOME/.tshrc
```

The file supports only:

```text
CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool
```

Inside an agent terminal, the runtime-provided process `CTX_PATH` remains
authoritative because it is part of the agent view.

It is not shell syntax and must not execute code.

`load` means tool metadata is added to the agent's current tool context.
`pin` means the tool is loaded and protected from automatic context eviction.
Eviction policy may unload unpinned tool metadata, never authority controls.

Interactive host-like behavior is provided by ordinary tool objects named
`bash`, `tmux`, or `zellij` when visible and allowed. `tsh` itself must not
fallback to arbitrary host commands.

## Prompt Runtime Contract

The first model system message is rendered from:

```text
agent/<name>.d/prompt.template.md
agent/<name>.d/system.md
project and global AGENTS.md files
bounded skill metadata
tool-injected context
optional historical message context
current time variables
immutable CortexFS runtime contract
```

Skill metadata contains `name`, `description`, and `SKILL.md` path. Full
`SKILL.md` is read only after the skill is selected. Skill metadata may use at
most 2% of the context window; when the context window is unknown, the hard cap
is 8,000 characters. Descriptions are shortened first; if still too large,
skills are omitted and a warning is included.

The runtime contract must tell the model:

```text
Your only native callable tool is tsh.
Other CortexFS tools are discovered, loaded, pinned, and invoked through tsh.
Prompt text and skill metadata do not grant permissions.
```

Humans can inspect the currently renderable system prompt with:

```text
ctx agent prompt <agent>
```

This command renders `system.md`, `prompt.template.md`, and the immutable
runtime contract through the same prompt renderer used by model execution. It
also collects currently discoverable `AGENTS.md` rules and bounded skill
metadata through the same library functions used by the object runner.
Runtime-only blocks such as tool injection and historical message context
remain bounded dynamic inputs; when they are not available to the CLI, the
command prints explicit placeholder text.

## Design Tests

The runtime design is healthy when all of these are true:

```text
agent.sh contains no protocol implementation beyond resolving ctx and execing ctx agent
ctx agent chat is the default human chat UI
ctx agent watch is the read-only human path into ctxterm -> tsh
ctx agent attach is the writable human path into ctxterm -> tsh
tsh never falls back to host PATH
default terminal cwd is /workspace
default terminal HOME is /home/agent
.git is read-only inside the default workspace mount
service/provider secrets are not inherited by executable agents
prompt text cannot grant tool, model, filesystem, network, or session authority
```
