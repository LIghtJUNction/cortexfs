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

`ctx agent chat` is the preferred human chat UI. `ctx agent repl` is a
compatibility alias for the same UI. They own line editing, `Ctrl+C`, socket
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

Executable agents select their launch ABI through the optional
`agent/<name>.d/abi` control. Absence means the exact legacy `argv-v1`
contract. `sdk-envelope-v1` opts into a host-written, bounded typed invocation
envelope on stdin and permits the host to restart the executable with the
authoritative result of a yielded tool call. `argv-v1` and `sdk-envelope-v1`
are the only v1 values.

The `sdk-envelope-v1` stdin body is exactly one UTF-8 JSON object followed by
one newline, with no other bytes, and is at most 1 MiB including that newline:

```json
{
  "schema": "cortexfs.agent-invocation/v1",
  "run": "run-1",
  "step": 1,
  "input": "original user input",
  "history_messages": "[]",
  "tool_context": "",
  "observation": {
    "tool_call_id": "call-1",
    "name": "example.echo",
    "status": "ok",
    "content": "authoritative normalized result",
    "truncated": false
  }
}
```

Unknown or missing fields are invalid. `run` and `step` must equal the
host-owned launch environment. Step 0 requires null `observation`; later steps
require exactly the immediately preceding host result. Context strings are at
most 64 KiB each and observation content at most 16 KiB. The host permits at
most eight calls and nine process starts, rejects replayed call ids before
authorization, rechecks policy for every call, and checks cancellation before,
during, and after each SDK/tool process. Only the host writes tool results and
the logical run's lifecycle frames. It records the original user message once,
each normalized result once, and one final assistant/error outcome; a process
crash cannot resume from agent-provided state.

An executable Agent SDK step may terminate by yielding exactly one typed
`tool_call`. The process emits no `done` frame and exits. The socket host
validates and executes the request through the existing agent tool authority,
policy, and sandbox path, emits the matching `tool_result`, and, for
`sdk-envelope-v1`, may start the next bounded step with that result in the
typed envelope. The host alone emits the logical run's final `done`.
Agent-originated results, malformed or multiple calls, and frames after a
yielded call are invalid output.

The optional `agent/<name>.d/approval` control is `auto` when absent and accepts
`auto` or `ask`. `ask` is valid only with `sdk-envelope-v1`. After the host has
completed direct-native declaration, path, agent/tool policy, Linux/mount, and
nofollow executable checks—but before spawn—it emits a bounded
`approval_request`. It reads exactly one bounded response on the same socket:

```json
{"op":"approve","run":"run-1","id":"call-1","decision":"allow_once"}
```

Only `allow_once` executes that prepared call. `deny`, EOF, timeout, malformed,
or mismatched responses fail closed and become host-owned approval and tool
result facts. Agent executables cannot emit approval frames.

The root-authoritative system socket accepts the agent owner UID or UID 0 for
internal child dispatch and stop. This UID 0 exception does not apply to the
receipt-bound per-run capability socket, which remains owner-UID only.

Before invoking an executable agent for a durable `send`, the socket runtime
sets `CTX_AGENT_HISTORY_MESSAGES` from the selected session's bounded
`messages.jsonl` history. This is prompt context only; it does not grant
additional session authority.

If a human sends `SIGINT` while a run is active, `ctx agent chat` sends a
`cancel` request for the active run id and returns to the prompt. In an idle
interactive chat, `Ctrl+C` exits the chat UI.

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

`ctx agent attach` should try the ABI path, then the user runtime path, then
the legacy runtime path.

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
agent/<name>.d/window
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

## Context Window Control

Every Agent, including a dynamically created child, has one durable setting:

```text
agent/<name>.d/window
```

The file contains exactly one canonical LF-terminated line:

```text
auto
```

or a positive base-10 `u32` token count. Numeric text has no sign, surrounding
whitespace, or leading zeroes. Zero, overflow, missing values, and extra lines
are invalid. Writing `auto\n` is the reset operation: it clears the explicit
override and returns the Agent to model-derived behavior. Reset does not alter
session history or context files.

`window` stores the setting, not a stale copied maximum. Its effective value is:

```text
auto       selected execution candidate's known model limit, otherwise unknown
number     that exact number
```

An explicit number is valid only when the selected model maximum is known and
the number is not greater than that maximum. Changing `model` must atomically
reject a state in which the existing explicit `window` is greater than the new
model maximum or the new maximum is unknown. It must not silently clamp or
reset the setting.

Fallback selection re-evaluates the same invariant for each candidate. With
`auto`, the effective value follows the actual candidate's known maximum. With
an explicit number, a fallback whose maximum is unknown or smaller is
ineligible and produces an auditable candidate error rather than silently
changing the Agent setting.

When the effective value is known, the host supplies its decimal token count
as the runtime-owned `CTX_CONTEXT_WINDOW_TOKENS` environment value. Existing
character-based prompt budgeting receives `CTX_CONTEXT_WINDOW_CHARS`, derived
with the conservative estimate of four UTF-8 characters per token. This
conversion is only a bounded prompt-budget estimate; it does not change the
token unit of `window` or `limit`. Arithmetic saturates at the receiving
budget's bound.

The host reserves `min(4096, max(1, effective_tokens / 4))` output tokens.
Prompt input admission therefore uses `effective_tokens * 4` total characters
minus four characters for every reserved output token. The exact rendered
Agent message array is serialized as JSON and its UTF-8 byte length is charged
conservatively against that character budget before every model dispatch.

The effective window bounds the complete prompt working set assembled for the
model call. Skill metadata keeps its existing 2% share of that derived
character budget. History, tool context, rules, system text, current input,
and output reservation must be accounted for before dispatch; durable raw
history is never deleted to meet this bound. When the effective window is
unknown, `CTX_CONTEXT_WINDOW_TOKENS` and `CTX_CONTEXT_WINDOW_CHARS` are absent
and the documented conservative legacy sub-budgets apply without claiming a
known model maximum.

Provider output controls such as Anthropic `max_tokens` remain separate. They
must not be derived by treating the combined context window as an output-token
limit.

## Tool Shell Contract

Agents see one native callable tool by default:

```text
tsh
```

An agent's optional `.d/tools` control may statically declare additional
direct-native tool names. Those names remain subject to fresh path, agent
policy, tool policy, mount, Linux permission, schema, and nofollow checks on
every call. Other tools are dynamically discovered, loaded, pinned, and
invoked through `tsh`; dynamic tsh cache state never expands the direct-native
set.

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

When an agent run builds its prompt, the object runner also writes session
load snapshots (best-effort) under the private agent session directory:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/AGENTS.md
/ctx/home/<uid>/agent/<agent>/session/<session>/SKILLS.md
```

```text
AGENTS.md  effective merged rules snapshot (same text as {{rules}})
SKILLS.md  skill metadata snapshot only (name, description, path)
```

These files are ordinary observability snapshots, not control or authority
files. Full skill content is not inlined; the agent opens listed `SKILL.md`
paths when needed. Snapshot writes must not fail the model run.

## Design Tests

The runtime design is healthy when all of these are true:

```text
agent.sh contains no protocol implementation beyond resolving ctx and execing ctx agent
ctx agent chat is the default human chat UI; ctx agent repl is only a compatibility alias
ctx agent watch is the read-only human path into ctxterm -> tsh
ctx agent attach is the writable human path into ctxterm -> tsh
tsh never falls back to host PATH
default terminal cwd is /workspace
default terminal HOME is /home/agent
.git is read-only inside the default workspace mount
service/provider secrets are not inherited by executable agents
prompt text cannot grant tool, model, filesystem, network, or session authority
```
