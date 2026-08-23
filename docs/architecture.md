# CortexFS Architecture

Normative ABI detail lives under [spec/](spec/). Visual identity lives in
[DESIGN.md](DESIGN.md) (Google Labs DESIGN.md format). This file is the
engineering design entry: what CortexFS is, where state lives, and what must
not become an additional root ABI.

## One-page model

```text
/ctx is a FUSE filesystem interface for agent runtimes.
model is a pure inference file.
agent is the policy-bound orchestrator.
tool is a capability endpoint.
channel is a filesystem-backed communication capability namespace.
session is ordinary file history.
policy is a minimal SELinux-like allowlist.
CortexFS protocol adapters remove provider and API-format differences.
CortexFS does not express provider/API formats as root ABI.
MCP servers are tool sources; MCP capabilities are ordinary tools.
CortexFS controls agent visibility, execution, and sharing, not framework config formats.
```

## Frozen root rule

```text
root only contains stable object classes; channel is the explicit communication root
root never mirrors provider, database, workflow, memory, or orchestration internals
MCP must not become a root namespace
MCP configs, skills, project rules, and prompt packages are ordinary visible files
```

Forbidden root namespaces (examples):

```text
skill/  memory/  mcp/  workflow/  chan/  job/  hook/  audit/  control/
```

Those concepts may exist as object-local files, session data, or tools. They
must not become new root classes.

## Core invariants

```text
Context is a working set, not the full history.
Raw history is durable.
Prompt context is disposable and rebuildable.
Compaction must not destroy raw messages.
Independent tasks should run in child agents.
Child agents are owned by their parent unless explicitly detached by policy.
Owned children die when the parent dies.
Prompt text and skill metadata never grant authority.
Policy, path, mount, uid/gid, and mode bits grant authority.
Mechanism enforces principal, path, mount, and Linux constraints; an injected
policy evaluator may only further restrict that authority.
```

## Architectural elegance

CortexFS keeps a Unix filesystem ABI and Linux authority model that coding
agent toolkits do not. Its **internal elegance bar** still matches the Pi
toolkit ([badlogic/pi-mono](https://github.com/badlogic/pi-mono)): strict
layers, a minimal tool loop, event facts instead of UI decisions, packages
usable alone, and extension without a second framework. What you leave out
matters as much as what you ship.

### Two mental models

| Mental model | Owns | Must not own |
| --- | --- | --- |
| Agent core | model turn, tool calls/results, cancellation, run events, context projection | TUI layout, channel SDK, FUSE projection, provider wire dialects |
| Interactive / host surfaces | `ctx` / `tsh` / `ctxchat` / `ctxterm`, channel adapters, web hosts | a second agent loop or parallel root ABI |

The same core must remain embeddable behind terminal, print, JSON/RPC-style
socket clients, and channel bridges. A new frontend adapts the existing
interaction contract; it does not fork the loop.

### Layered package map

Pi’s stack is `ai → agent-core → coding-agent (+ tui)`. CortexFS maps the same
gravity onto Rust crates and processes:

```text
Application / UX     ctx, tsh, ctxchat, ctxterm, channel adapters, web hosts
        ▲
Agent core           agent runtime + object runner (loop, tools, policy gate)
        ▲
Protocol / AI        cortexfs-protocol, provider registry, model projections
        ▲
Foundation           abi types, support fs/jsonl/layout, module contract, paths
```

| CortexFS gravity | Pi analogue | One job |
| --- | --- | --- |
| `cortexfs-protocol` | `pi-ai` | provider-neutral request/event IR; no HTTP, secrets, or loop |
| `cortexfs-module` + runner loop | `pi-agent-core` | lifecycle, capabilities, turn/tool mechanics |
| `cortexfs-runtime-client` | agent event/API surface | `cortexfs.interaction/v1` for every frontend |
| `ctx` / terminals / channels | `pi-coding-agent` / `pi-mom` | sessions, UX modes, platform adapters |
| FUSE `/ctx` projection | *(CortexFS-specific)* | inspectable object classes; not an AI DB mirror |

Lower layers never import upper ones. Protocol code must not know agents.
Agent-core code must not know TUI widgets or Discord payloads. Channel
adapters translate platform frames and stop at the interaction/channel
socket boundary.

### Minimal loop, durable facts

The executable core stays the same small feedback loop:

```text
build disposable context from durable session facts
  → stream model turn
  → collect tool calls
  → authorize + execute tools
  → append observations
  → repeat until final answer or cancel
```

Everything else is layered outside that loop:

```text
skills / rules / templates     → context inputs, never authority
extensions / modules / MCP     → tools or adapters, never root classes
approvals / sandbox / policy   → gates around the same tool path
compaction / summaries         → rebuild prompt; never rewrite raw history
frontends                      → subscribe to events; never own the loop
```

Session files answer “what happened?” Prompt context answers “what does the
model need next?” Those objects stay separate, as in Pi’s session tree versus
`convertToLlm` projection.

### Events are facts

Every layer emits typed, correlatable facts (`run`, `request_id`, tool id,
status). Terminals render them, JSON clients serialize them, session recorders
append them, tests assert order. Presentation never feeds back into authority
or history schema. Interaction and channel sockets already follow this rule;
new surfaces must reuse those event families instead of inventing parallel
control planes.

### Composability and omission

Packages must stay independently useful:

```text
cortexfs-protocol alone     → transcode provider formats
runtime-client alone        → speak interaction frames
tool-sdk / agent-sdk alone  → implement one capability process
channel-sdk alone           → isolate one platform transport
```

Deliberate omissions (the anti-framework):

```text
no workflow / hook / job / memory root
no plan-mode product surface baked into the loop
no provider dialect in /ctx paths or agent branches
no in-process mega-harness that loads every channel SDK
no background watchers or hot-reload control plane
```

Specialization belongs in objects, modules, skills, and adapters. The host
keeps stable primitives: files, sockets, policy, atomic rename, and process
restart.

### Extension points (anti-framework)

Pi extends at the AI, agent, and application layers without growing a plugin
root. CortexFS uses the same idea with Unix boundaries:

| Layer | Extend with | Must not become |
| --- | --- | --- |
| Protocol / AI | provider adapters, `cortexfs-protocol` routes, model `driver` / `cap` projections | `/ctx` provider dialect paths or agent `match` on vendor names |
| Agent core | `cortexfs-module` lifecycle, Tool/Agent SDKs, policy evaluators, context transformers | in-loop plan boards, hook DAGs, or a second orchestration ABI |
| Application | one-file packages ([extensions.md](extensions.md)), skills/rules files, channel adapters, terminal/web/IM clients | `/ctx/skill`, `/ctx/mcp`, `/ctx/workflow`, or resident plugin daemons |

Concrete surfaces already in the tree:

```text
cortexfs.module.socket/v1     process-isolated module lifecycle
Tool SDK / Agent SDK          one executable capability or agent step
cortexfs.package/v1           authoring input → ordinary agent/tool objects
cortexfs.interaction/v1       every frontend speaks the same request/event facts
cortexfs.channel.socket/v1    platform adapters stay outside the agent loop
skills / AGENTS.md / rules    disposable context inputs, never authority
MCP via ctxmcp                ordinary tools; never a root class
```

The structure ends at stable primitives and lifecycle edges. New behavior is a
new object, module, skill, or adapter—not a new root directory and not a
hot-loaded in-process extension host. See [module-abi.md](spec/module-abi.md)
and [extensions.md](extensions.md).

## Identity, lifetime, and transport

CortexFS uses four different identities. They must not be collapsed into an
"agent daemon" or duplicated in a second lifecycle tree:

| Layer | Stable identity | Owner |
| --- | --- | --- |
| Definition | `agent/<name>` + `agent/<name>.d/` | reference tree |
| Runtime instance | supervisor unit + invocation receipt | runtime/supervisor |
| Session | `home/<uid>/agent/<name>/session/<session>/` | durable files |
| Run | entropy-backed run id in session events | session recorder |

The definition says how an Agent may run. A runtime instance says which
processes currently realize that definition. A session owns durable human and
Agent history. A run correlates one bounded execution inside that session.

`agent/<name>.d/meta.json` may retain the latest receipt-bound supervisor facts
needed for inspection and safe cleanup. `status`, `pid`, and `log` are summary
projections. None of those files changes the Agent's definition identity, and
none is an independent process supervisor.

Do not add `instances/` merely to mirror process state already owned by systemd
and receipt metadata. A future multi-instance feature must first define an
identity that cannot be expressed by the existing agent/session/unit/run tuple,
then nominate one authoritative lifecycle owner and migration path.

Sockets are transports. Live sockets belong under `/run`; paths such as
`agent/<name>.sock` and `session/<session>/terminal/main.sock` are stable ABI
entries or aliases used to discover those transports. Socket presence does not
define object identity, session durability, or process ownership.

Frontend interaction uses the existing Agent/session socket as its canonical
runtime boundary. `cortexfs-runtime-client` names that logical contract
`cortexfs.interaction/v1`; terminal, web, and IM clients share request/event
semantics and correlation ids. `cortexfs-channels` has a separate
`cortexfs.channel.socket/v1` driver boundary for platform lifecycle, delivery,
receipts, live effects, and tool control. The `/ctx/channel/<name>` tree
exposes only generic channel state and tools; platform-specific message types
do not cross the Agent boundary.

Heavy or OS-specific transports remain external processes on that boundary.
For example, `cortexfs-channel-nostr` owns relay WebSockets and NIP-04/NIP-17
cryptography while the core runtime only sees provider-neutral channel frames.
This keeps a small host from loading every platform SDK into one process and
makes per-channel restart and memory limits explicit. Agent terminals and
socket-activated runtimes likewise carry hard cgroup ceilings (`MemoryMax`,
`CPUQuota`, `TasksMax`) so one sandboxed agent cannot exhaust the host.

A terminal is a durable resource below a session. Its resource directory owns
metadata and replayable events; a runtime PTY and socket are replaceable
mechanisms for the process and attachments. The first terminal resource slice
uses:

~~~text
home/<uid>/agent/<agent>/session/<session>/terminal/<terminal-id>/
  meta.json  state  status  owner  cwd  events.jsonl
~~~

The root remains frozen: this session-local path does not add /ctx/terminal.
A top-level terminal class requires a separately versioned root ABI decision.

Interactive frontends use the same session-local discovery rule. Each durable
terminal, web, or external channel is represented by a filename below
`home/<uid>/agent/<agent>/session/index/channel/` (or the corresponding
shared-space session index). The filename is the user-facing channel selector;
its JSON content only maps a generic transport to an existing agent/session.
This keeps `ctx attach` discoverable with `ls` while preserving the frozen root
ABI and the existing interaction socket.

The compact rule is:

```text
object defines identity
supervisor receipt defines process lifetime
ordinary files define durable state
socket provides optional transport
terminal resource owns PTY history; socket is only a live transport
```

Agent session runtime state has two compatible projections. The existing
single-line `state` file remains the lifecycle compatibility surface; the
optional `state.json` file is a structured, non-secret view of status, phase,
run, step, selected model, context revision, and stable error code. Runtime
transitions update both through the existing atomic file replacement helper.
Clients inspect that projection through the agent/session socket's `status`
request and replay facts through `resume`; no watcher or second root control
plane is introduced.

During a hosted Agent step, `context_revision` is a length-delimited SHA-256
digest of the bounded history, tool context, and previous observation inputs.
It lets a status client detect a rebuilt working set without exposing prompt,
message, tool-result, or credential contents.

## Model and context boundary

A model object is the stable provider/model identity. Its `driver` control
selects replaceable adapters for each use case; `cap`, `limit`, `recommended`,
and `compact` project only provider-neutral facts. Agents and context code
consume those projections and must not branch on provider names, API formats,
or model branding.

Capability data is conservative. Hard limits use the precedence defined by the
Model ABI: explicit per-model host configuration, then the validated catalog,
then `unknown`. Stable `cap` words are adapter projections; unsupported or
untrusted facts are omitted. A future per-model capability override or
host-side probe requires a versioned Model ABI change. It must not become a
model-call side effect, a background watcher, or a second configuration store;
accepted evidence would enter the same validated `cap`/`limit` projection or
remain diagnostic-only.

Context construction uses the selected model's hard `limit`, metadata
`recommended`/`compact` policy, and the Agent's attenuating `window`/`compact`
controls. `window=auto` follows the model recommendation, not necessarily the
maximum advertised window. Raw session history remains intact while the
rendered prompt may use a recent tail, summaries, rules, skills, and loaded tool
metadata. Changing models therefore rebuilds prompt context from durable facts;
it does not rewrite history or teach each Agent a table of model-specific cases.

## Runtime module boundary

`cortexfs-module` is the shared static Rust module API plus the versioned
external module wire contract. It defines provider-neutral metadata,
capability declarations, executor-independent async lifecycle methods, and a
deterministic static registry for Agent, Tool, Channel, Model, and Context
modules. The crate is independent of FUSE, `/ctx`, provider protocols, and
runtime storage; domain SDKs add their typed behavior above this boundary.

The Rust trait API is intentionally statically composed. A Rust trait object is
not promised as a `cdylib` ABI. The recommended third-party boundary is the
`cortexfs.module.socket/v1` JSONL-over-Unix-socket contract, which preserves
compiler and allocator independence and adds process isolation while keeping
the same lifecycle and capability model.

## Where things live

The packaged host keeps versioned durable trees under
`/var/lib/cortexfs/storage/generations/<generation>` and exposes the selected
tree through the atomic `/var/lib/cortexfs/storage/current` symlink. On a
systemd restart, `ctx storage update` clones the current generation, applies
and validates the next `bin/cortexfs.bootstrap.json` `tree_version`, then
switches `current`. A failed stage leaves `current` unchanged. This is a
restart boundary, not a watcher, poller, or hot reload;
the `/ctx` ABI shape remains unchanged. The package generates root files
locally; generations are not distributed artifacts. The systemd restart path,
after stopping consumers, explicitly uses `--prune` to remove non-current
generations. There is no background generation GC.
The mount and agent runtime resolve `current` once at process startup and keep
that concrete generation for their full lifetime, including mount cache
refresh. Short-lived object-runner invocations may resolve the then-current
generation each time.

| Place | Path shape | Role |
| --- | --- | --- |
| Control | `/ctx/agent/<name>.d/*` | policy, mount, cwd, system.md, loop |
| Agent home | `/ctx/home/<uid>/agent/<name>/` | session, data, cache, log |
| Session | `.../session/<session>/` | messages, events, state.json, context, load snapshots |
| Runtime IPC | `/run/user/<uid>/cortexfs/...`, `/run/cortexfs/terminal/broker.sock` | Agent sockets and the root terminal broker |

Sandbox mapping (typical):

```text
/ctx/home/<uid>/agent/<name>  →  HOME=/home/agent   (rw)
caller project cwd            →  /workspace         (rw, default cwd)
/ctx                          →  /ctx               (often ro)
```

`/run` holds sockets. The root-owned terminal broker authenticates operators
and passes accepted descriptors to sandboxed supervisors; it does not relay
PTY bytes. Agent cwd is usually `/workspace`. Private session files
live under agent home, not under `/run`.

## Prompt load observability

When the object runner builds a run prompt, it best-effort writes:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/AGENTS.md
/ctx/home/<uid>/agent/<agent>/session/<session>/SKILLS.md
```

```text
AGENTS.md   merged rules snapshot (same text as {{rules}})
SKILLS.md   skill metadata only (name, description, path)
```

Ordinary session files, not authority. Full skill bodies stay at listed
`SKILL.md` paths. Implementation: `agent/prompt/snapshot.rs`.

## Engineering taste

Derived from the elegance bar above:

```text
short names over long phrases
one clear job per module and per crate
reuse before inventing helpers
no parallel enums for Empty/Missing/Invalid
no second root ABI for orchestration; channel state/tools use the explicit root
no background watchers, polling, or hot-reload subcommands
Git commit (or process restart) is the development refresh boundary
atomic rename for control-plane writes
ordinary files for history and snapshots
lower layers never import upper layers
events are facts; UIs only subscribe
leave complexity out of the loop until a stable boundary requires it
```

Module naming: [naming-guide.md](naming-guide.md). Prefer single-token stems
(`snapshot.rs`); no new `-` / `_` in module file stems.

## Internal code architecture

Product rules above freeze **what** `/ctx` is. How the Rust tree is layered
(process roles, crate/feature splits, module dependency direction, error
tiers, migration phases) lives in
[internal-architecture.md](internal-architecture.md).

Read that document before large refactors (crate splits, executor error
migrations, FUSE vs object boundary changes). Do not “improve structure” by
adding root ABI classes, workflow engines, or background watchers.

## Read the specs in order

```text
architecture.md              # elegance bar, extension points
internal-architecture.md     # crate/module layers
spec/README.md
spec/root-abi.md
spec/fuse.md
spec/object-abi.md
spec/model-abi.md
spec/session-abi.md
spec/agent-tool-security.md
spec/agent-runtime.md
spec/module-abi.md
spec/tool-policy-abi.md
spec/ctx-coreutils.md
spec/rolling-upgrades.md
extensions.md
```

## Stable ABI red line

```text
Do not let /ctx become a directory mirror of an AI platform database.
It should stay small, hard, boring, and scriptable.
Match Pi’s elegance internally without importing Pi’s product surface as root ABI.
```
