# Agent Runtime

This document defines the stable end-to-end agent runtime shape. It ties together
the agent object ABI, the human CLI, socket sessions, the persistent terminal,
the `tsh` tool shell, prompt construction, and sandbox execution.

It does not add a root namespace. Everything here is derived from existing stable
objects and files.

The durable terminal resource used by this runtime is specified in
[terminal-abi.md](terminal-abi.md). In this first slice an Agent start
materializes one terminal resource below its session; the resource id and event
history remain after the live PTY socket exits.

## Definition, Instance, Session, And Run

The word `agent` does not name one daemon. The runtime distinguishes four
layers:

```text
definition  /ctx/agent/<name> + /ctx/agent/<name>.d/
instance    supervisor unit and receipt-bound invocation
session     /ctx/home/<uid>/agent/<name>/session/<session>/
run         one run id recorded in that session's event stream
```

An Agent definition is durable configuration and policy. Starting it may create
more than one process role: the system Agent socket service and an optional
per-session terminal unit. Those processes are runtime instances; their unit,
invocation, pid, identity, socket, and session facts are bound by launch
receipts. The latest cleanup receipt may be projected in
`agent/<name>.d/meta.json`, while durable `agent.start` and run events preserve
correlation facts in ordinary logs and session history.

A session is not a process and survives instance exit. A run is not a session
and cannot silently acquire authority from previous runs. Restarting a runtime
may resume a private or shared session without preserving a process identity.

There is deliberately no parallel `instances/` control plane. systemd owns live
process truth, receipt metadata proves which generation CortexFS may stop, and
session files own durable history. Copying those facts into another mutable
directory would introduce conflicting lifecycle authorities.

The Agent socket and terminal socket are transports for an active instance.
Their stable `/ctx` paths may be aliases to endpoints under `/run`, and their
absence never deletes the Agent definition or a private/shared session.

The packaged `cortexfs-agent@.socket` is ordered after and restart-coupled to
`cortexfs.service`. Package upgrades also start only the agent socket instances
explicitly enabled through `sockets.target.wants`; an upgrade therefore does
not leave an enabled chat endpoint inactive after the FUSE mount restarts.

## Runtime Surfaces

The equivalent resource commands are ctx terminal status, ctx terminal watch,
and ctx terminal attach. ctx terminal create AGENT is currently an agent-backed
compatibility create: it starts the Agent session and records the terminal
resource. A detached command supervisor is reserved for a later terminal ABI
revision.

There are three separate surfaces:

```text
human chat       ctx agent chat/send/resume/cancel
human terminal   ctx agent watch/attach
agent tool use   tsh inside ctxterm
```

They must not collapse into one interface.

`ctx agent chat` is the human chat UI. It owns line editing, `Ctrl+C`, socket
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

Every tool execution also intersects `agent/<name>.d/perm`: `r` admits the
built-in read/list/stat family, `w` admits write/replace, and `x` admits shell
and host-like terminal tools. `tsh` remains the routing shell and does not by
itself consume `x`; the selected capability does.

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
{"op":"tsh","id":"tool-1","session":"default","args":["load","bash"]}
{"op":"resume","session":"default"}
{"op":"status","session":"default"}
{"op":"cancel","id":"run-1"}
```

`status` is a read-only control-plane query. It returns one bounded JSONL
projection such as:

```json
{"type":"status","session":"default","status":"active","phase":"active","run":"run-1","step":1,"model":"main","updated_at":"1710000000"}
```

The projection is non-secret. It may contain lifecycle, run, step, action,
tool, model reference, context revision, timestamp, and stable errno fields,
but never credentials, prompt contents, or message contents. A missing session
is reported as `idle`; malformed state is an `EIO`-class runtime failure.
When a hosted step has a compiled working set, `context_revision` is a
length-delimited SHA-256 digest of its bounded inputs; it is not the Context
contents and cannot be used to reconstruct them.
Existing clients can continue to use `resume` for replay and `send` for a live
run stream.

The socket surfaces map to Unix-style operations without adding a root
namespace: chat is `send`, command execution is `tsh` or `cancel`, event
attachment is `resume` with an optional cursor, and inspection is `status`.
Each request remains bounded and one-shot at the transport layer; a caller
may reconnect with the last event id after a disconnect. Durable files, not
socket presence, remain lifecycle truth.

`tsh` executes through the authenticated agent runtime without a model call.
It emits canonical `start`, `tool_call`, `tool_result`, and `done` frames.
Repeating an identical request id replays the durable result without executing
the command twice.

The socket runtime records user messages before invoking the agent/model path.
Assistant text is derived from stable event frames and recorded back to the
durable session. Raw messages and events remain ordinary files; context packs
are rebuildable views.

Executable agents use the required `agent/<name>.d/abi` control. Its accepted
value is `sdk-envelope-v1`: the host writes a bounded typed invocation envelope
to stdin and may restart the executable with the authoritative result of a
yielded tool call.

The `sdk-envelope-v1` stdin body is exactly one UTF-8 JSON object followed by
one newline, with no other bytes, and is at most 1 MiB including that newline:

```json
{
  "schema": "cortexfs.agent-invocation/v1",
  "run": "run-1",
  "step": 1,
  "input": "original user input",
  "origin": {
    "transport": "channel",
    "endpoint": "discord",
    "conversation": "room-1",
    "thread": "thread-1"
  },
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
most 64 KiB each and observation content at most 16 KiB. The host reads the
canonical positive `CTX_AGENT_STEPS` value from `agent/<name>.d/env` as the
per-run tool continuation budget; bootstrap writes 64. The derived value is
informational to the executable Agent, while the host enforces the budget. The
host rejects replayed call ids before
authorization, rechecks policy for every call, and checks cancellation before,
during, and after each SDK/tool process. Only the host writes tool results and
the logical run's lifecycle frames. It records the original user message once,
each normalized result once, and one final assistant/error outcome; a process
crash cannot resume from agent-provided state.

An executable Agent SDK step may terminate by yielding exactly one typed
`tool_call`. The process emits no `done` frame and exits. The socket host
validates and executes the request through the existing agent tool authority,
policy, and sandbox path, emits the matching `tool_result`, and may start the
next bounded step with that result in the typed envelope. The host alone emits
the logical run's final `done`.
Agent-originated results, malformed or multiple calls, and frames after a
yielded call are invalid output.

For a channel-backed run, Runtime additionally injects these child-only
values:

```text
CTX_CHANNEL_ID
CTX_CHANNEL_SESSION
CTX_CHANNEL_CAPS
CTX_CHANNEL_SOCKET
```

`CTX_PATH` is rebuilt for the request in this order: the user's channel tool
directory, the global channel tool directory, the Agent's original path, then
the normal `/ctx/tool` tiers. User tools override global channel tools; a
collision with an existing Agent tool is rejected. The channel token and other
credentials never enter these values. Identity, attachments, and thread data
stay in the bounded structured `origin`/`event` envelope.

The optional `agent/<name>.d/approval` control is `auto` when absent and accepts
`auto` or `ask`. In `ask` mode, after the host has completed direct-native
declaration, path, agent/tool policy, Linux/mount, and
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

Before invoking an executable agent, the socket runtime writes exactly one
`sdk-envelope-v1` frame to stdin. Its `history_messages` and `tool_context`
fields carry bounded prompt context; the agent boundary does not expose legacy
`CTX_AGENT_HISTORY_MESSAGES` or `CTX_AGENT_TOOL_CONTEXT` environment inputs.
When a run makes several tool calls, `tool_context` retains earlier authoritative
observations in order while `observation` carries the immediately preceding
result. The host keeps this transcript bounded by the tool-context limit. The
built-in hosted agent converts the latest matched call and observation into
canonical assistant-tool-call and tool-result messages for OpenAI Chat and
Responses continuation. The call IDs must match; missing or malformed metadata
falls back to bounded text context and never invents a tool result.

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
  -> ctxterm --broker AGENT SESSION UNIT -- /ctx/bin/tsh
  -> register and activate with the root broker
  -> tsh
```

`ctxterm` owns the pseudo-terminal. The root broker authenticates `watch` and
`attach` clients and passes accepted descriptors directly to `ctxterm`; it does
not relay PTY bytes.

Session terminal sockets are visible through the ABI path:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

The visible entry aliases the immutable root-owned endpoint
`/run/cortexfs/terminal/broker.sock`. `ctx agent attach` sends a bounded v1
broker request containing the agent, session, mode, and fresh nonce. It MUST
NOT fall back to a per-user socket or the legacy line protocol. See
[terminal-broker.md](terminal-broker.md).

## Sandbox Contract

`ctx agent start` creates the default interactive terminal sandbox. Unless
overridden, it binds the caller's current working directory at `/workspace` with
read-write access and starts the terminal there. If the host directory contains
`.git`, `.git` is over-mounted at `/workspace/.git` read-only.

Host-native terminals are rejected before session or launch state changes.
A same-UID native agent cannot be distinguished reliably from its operator at
the terminal boundary. Native mode may return only after such agents receive a
distinct Unix identity and an equivalent broker authorization path.

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

Unless an explicit mount replaces it, `/tmp` is a private sandbox tmpfs capped
at 512 MiB. It is never the host `/tmp`; exceeding the limit fails the write
inside the sandbox rather than consuming unbounded host storage.

Every sandbox also applies host isolation flags before process mounts:

```text
--as-pid-1
--new-session
--cap-drop ALL
--unshare-uts
--hostname cortexfs
```

Interactive terminals launched by `ctx agent start` are transient user systemd
units with hard cgroup ceilings (see `support::quota`):

```text
MemoryMax=1G
MemoryHigh=768M
CPUQuota=200%
TasksMax=256
LimitNOFILE=1024
OOMPolicy=stop
```

The host refuses to start another terminal when eight agent terminal units are
already running for that user. Socket-activated agent runtimes use a stricter
`MemoryMax=512M` / `CPUQuota=100%` / `TasksMax=128` profile, matching the
packaged `cortexfs-agent@.service` unit.

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

## Tool Workspace Overlay

When a Tool execution receives a valid `CTX_WORKSPACE`, CortexFS mounts the
project through the session's writable overlay. Its `upper` and `work` data
remain below the durable session; the backing project is the lower view. The
overlay includes `.git`, so a declared writable `/workspace/.git` mount must
not bypass that session view. Without `CTX_WORKSPACE`, CortexFS preserves the
declared mount table and creates no workspace overlay.

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
agent/<name>.d/compact
agent/<name>.d/policy
agent/<name>.d/uid
agent/<name>.d/gid
agent/<name>.d/groups
agent/<name>.d/perm
agent/<name>.d/label
```

Prompt text, `AGENTS.md`, skill metadata, `.mcp.json`, and tool descriptions
may influence model behavior, but they do not grant authority.

The reference tree reserves the Agent alias `main`:

```text
agent/main      -> agent/coder
agent/main.sock -> agent/coder.sock
```

An Agent alias is an entry-point link, not a second Agent or a second control
directory. The canonical control tree, Linux identity, permissions, runtime
receipt, and durable sessions remain owned by the target Agent. Alias targets
are fixed and validated by bootstrap; arbitrary Agent directory links are not
accepted as an authority boundary.

Actual authority is the intersection of:

```text
mount/chroot visibility
Linux uid/gid/groups and mode bits
agent `perm` capability ceiling
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

It has a second durable context policy setting:

```text
agent/<name>.d/compact
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
auto       selected model's metadata recommended window, bounded by limit
number     that exact number
```

`compact` uses the same syntax. Its `auto` value follows the selected model's
metadata compaction threshold and is bounded by the effective `window`; an
explicit number is a deliberate smaller threshold. The model's read-only
`limit`, `recommended`, and `compact` files remain the source of defaults, so
switching an Agent model does not leave stale copied values in `agent.d`.

An explicit number is valid only when the selected model maximum is known and
the number is not greater than that maximum. Changing `model` must atomically
reject a state in which the existing explicit `window` is greater than the new
model maximum or the new maximum is unknown. It must not silently clamp or
reset the setting.

Fallback selection re-evaluates the same invariant for each candidate. With
`auto`, the effective value follows the actual candidate's recommendation. With
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

When known, the effective compaction threshold is supplied as
`CTX_CONTEXT_COMPACTION_TOKENS`; the selected durable setting is available as
`CTX_AGENT_COMPACT_SETTING`. These values tell a context compiler when to
rebuild its working set; they never delete `messages.jsonl` or the `raw` view.

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

The permission control is re-read into the runtime view together with identity,
mount, path, and policy controls. Changing it affects subsequent executions;
it never expands policy, mount, or Linux authority.

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

These passthrough tools use terminal stdout/stderr rather than the Tool SDK
JSONL envelope. The runtime captures their bounded output and wraps it as the
provider-neutral tool observation; SDK-backed tools continue to require the
`start`/`message`/`done` envelope.

## Self Iteration

An agent iterates itself through the `agent.update` tool. The tool sends one
`agent.update` frame over the receipt-bound run capability socket:

```json
{"op":"agent.update","request_id":"tool-1","agent":"coder","session":"default","run":"run-1","control":"system.md","content":"..."}
```

The sandbox receives exactly `CTX_CONTROL_SOCKET=/run/cortexfs/control.sock` for
this channel, never a bearer credential. Before each bwrap launch, the host
records that host PID and `/proc/<pid>/stat` start time, then releases its
one-use `--block-fd` gate. The socket accepts only the owner UID whose kernel
peer PID is that registered launch root or a live descendant; missing process
state, PID reuse, reparenting, cycles, and excessive ancestry depth deny the
request. Legacy JSON `token` input remains parseable for wire migration but is
ignored; new clients omit it and must never derive authority from an
environment value.

The frame's `agent`, `session`, and `run` must equal the capability's own
identity, so the operation is self-only by construction. `control` accepts
exactly `system.md` or `prompt.template.md`; `content` is bounded UTF-8 text
of at most 8 KiB without NUL bytes. The host revalidates both and atomically
replaces (or creates, for an optional control not yet materialized)
`agent/<self>.d/<control>` in the backing source. The tool generates a fresh
request id per invocation; replaying an already-seen request id returns
`EALREADY` without writing twice. A request id is consumed by any authorized
attempt, successful or not — a failed attempt must retry under a new id.

The update takes effect at the next run: prompt construction reads `system.md`
and `prompt.template.md` fresh from the control directory for every run, and
prompt text grants no authority. Authority controls cannot travel through this
operation.

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

The optional `agent/<name>.d/loop` control selects the behavior contract passed
to an executable Agent through `CTX_AGENT_LOOP`. Built-in values are `chat`,
`react`, `coding`, `planner`, and `research`; a validated object name may name
a custom loop. This is a behavior hint, not a capability grant: the executable
still uses the unchanged `sdk-envelope-v1` stdin/stdout ABI, and tool authority
continues to come only from the runtime policy intersection.

Agent-local hooks use the existing `agent/<name>.d/hooks/pre.d/` and
`post.d/` directories. The runtime runs them in lexical order immediately
before and after each model action. A hook is an executable regular file and
receives one JSONL frame such as:

```json
{"abi":"cortexfs.hook/v1","phase":"pre","action":"model","agent":"coder","run":"run-1","step":0}
```

The frame is deliberately metadata-only. Hooks do not receive the current
prompt, user message, model response, provider secret, or socket capability.
Exit zero continues the run; a non-zero exit becomes a host-owned error frame.
The host applies the same agent Linux identity, rejects symlinks, bounds hook
count/output/time, and discards hook stdout/stderr after recording only the
stable error code. Hook directories are ordinary object-local files and are
reloaded at the next runtime/process boundary; no watcher or hot reload is
involved.

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
