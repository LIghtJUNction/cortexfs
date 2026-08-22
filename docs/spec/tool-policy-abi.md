# Tool, Shared, Policy, and Logs ABI

This file continues [agent-tool-security.md](./agent-tool-security). It keeps
the tool ABI, MCP projection rules, shared-space rules, policy v0, and log
placement separate from agent identity and mount setup.

## Tool as File

```text
/ctx/tool/
  fs.read
  fs.read.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
  fs.list
  fs.list.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
  fs.stat
  fs.stat.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
  fs.write
  fs.write.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
  shell.exec
  shell.exec.d/
    name
    description
    schema
    cap
    policy
    status
    log
    hooks/
      pre.d/
      post.d/
```

MCP servers are tool sources, not CortexFS root objects. Do not expose:

```text
/ctx/mcp/github
/ctx/mcp/figma
```

`ctxmcp` projects an explicitly selected external stdio server as ordinary
tools. Projection writes v2 manifest candidates only; it does not install,
grant policy, or write `/ctx`:

`ctxmcp` advertises stable MCP `2025-11-25` and accepts only the negotiated
stable versions `2025-11-25`, `2025-06-18`, `2025-03-26`, and `2024-11-05`.
Draft, unknown, and future versions are rejected.

```text
/ctx/tool/github.search_issues
/ctx/tool/github.create_issue
/ctx/tool/figma.get_file
/ctx/tool/chrome.open
```

The projected name is exactly `<server>.<remote_tool>` and must fit the normal
64-byte object-name grammar. There is no implicit `mcp.` prefix or registry.
The optional stable `mcp` control is a strict stdio locator containing
`transport`, an absolute visible external `config` path, `server`, and `tool`.
Projection never copies the external configuration or its secrets.

```bash
ctxmcp list --config "$HOME/.config/example/mcp.json" --server github
ctxmcp project --config "$HOME/.config/example/mcp.json" \
  --runtime-config /workspace/.mcp.json --server github --out ./mcp-manifests
ctx object check ./mcp-manifests/github.search_issues.manifest.json
ctx object install --source /var/lib/cortexfs/storage/current \
  ./mcp-manifests/github.search_issues.manifest.json --tier system
```

Installation remains the existing explicit `ctx object install` lifecycle and
does not authorize any agent.

MCP-backed capabilities may be projected as ordinary tools, but they are not
default built-ins. CortexFS does not define where MCP servers are configured.
The agent runtime or tool adapter may discover MCP servers from ordinary files
visible inside the agent view.

Projected tool control files remain the ordinary tool ABI:

```text
/ctx/tool/github.search_issues.d/schema
/ctx/tool/github.search_issues.d/policy
/ctx/tool/github.search_issues.d/status
/ctx/tool/github.search_issues.d/log
```

An implementation may expose an optional diagnostic origin file:

```text
/ctx/tool/github.search_issues.d/origin
```

`origin` is not stable ABI. Strict clients must not depend on it. MCP is only
where the tool came from; it is not a new namespace, policy class, submission
path, or CortexFS-defined server configuration format.

Tools are executable capability endpoints:

```bash
/ctx/tool/fs.read '{"path":"README.md"}'
echo '{"cmd":"pwd"}' | /ctx/tool/shell.exec
```

For an agent caller, the matching `agent/<name>.d/perm` bit is an additional
mandatory ceiling before tool policy: `r` for `fs.read`/`fs.list`/`fs.stat`,
`w` for `fs.write`/`fs.replace`, and `x` for shell or terminal execution.

Optional agent lifecycle tools may appear only when implemented:

```text
/ctx/tool/agent.create
/ctx/tool/agent.start
/ctx/tool/agent.stop
```

If `agent.create` exists, it must enforce attenuation. At minimum it checks:

```text
parent agent has policy permission to create the named child
requested child permissions are a subset of parent permissions
requested child mounts are a subset of parent-visible mounts
requested child groups are a subset of parent groups
requested child name is valid
```

Authorization is explicit and child-name-specific; the reference agents do
not receive it by default. A test or deployment policy for child `review-1`
uses both grants:

```text
allow parent_t tool:agent.create execute
allow parent_t agent:review-1 create
allow parent_t agent:review-1 start
```

## Agent Self Iteration

`/ctx/tool/agent.update` is the self-iteration endpoint. It lets a running
agent replace exactly one of its own authority-free prompt controls:

```text
agent/<self>.d/system.md
agent/<self>.d/prompt.template.md
```

The tool submits the update through the receipt-bound run capability socket.
The host binds the request to the calling agent, session, and run, so an agent
cannot name another agent. The host revalidates the control name and content,
rejects payloads larger than 8 KiB, and applies the replacement atomically in
the agent's own control directory. Every other agent control, including
`policy`, `mount`, `model`, `window`, and the identity files, stays host-owned
and is rejected with `EINVAL`.

Prompt text does not grant authority. A self update changes behavior only when
the next run renders its prompt; it cannot expand mounts, policy, tools, or
Linux identity. Like every tool call, the update is recorded as ordinary
`tool_call`/`tool_result` facts in the durable session, so self iterations are
auditable from session history alone.

Execution requires both layers, like any tool:

```text
allow <agent>_t tool:agent.update execute
```

in the agent policy and in the tool policy.

## Agent Terminal Tools

Agents should get one terminal capability: `tsh`. A runtime launches it inside
`ctxterm`, the pseudo-terminal owner:

```text
/ctx/bin/ctxterm
/ctx/bin/tsh
/ctx/tool/tsh
```

`ctxterm` starts `tsh` by default and owns the PTY for the whole agent terminal
lifecycle. `ctx agent start` launches that terminal inside a sandbox; by
default the caller's current directory is mounted at `/workspace` and the agent
starts there. `tsh` is not a host shell. It resolves the first word through
`CTX_PATH` and executes only the matching CortexFS tool object.

Tool execution has two caller-facing modes:

```text
terminal CLI     tsh TOOL ARG...
agent native     in-process/runtime tool call with structured input/output
```

The terminal CLI mode should behave like a normal command line program: argv is
preserved, stdin/stdout/stderr are inherited, and output is plain command
output. A tool may decide that empty argv is invalid, but `tsh` must not reject
empty argv for ordinary visible tools before the tool runs. The native agent
mode uses structured JSON input and JSONL tool frames. Executable plugins run
through the same authorized object path as other tools. The Tool SDK defines a
dynamic-library ABI, but the current core does not load it; `load` and `pin`
currently affect metadata context/cache and must not force terminal CLI
commands to emit structured frames.

Executing a tool through `tsh` requires an agent terminal context so CortexFS
can evaluate the agent identity, mounts, policy, and `CTX_PATH` together. A
standalone human `tsh` process may discover tools and inspect metadata, but it
must not fabricate an agent identity to execute tools.

`ctx tool NAME [ARG...]` may directly run only allowlisted safe CortexFS core
tool CLIs that are implemented inside `ctx`, such as `tsh.config`. It still
requires `NAME` to be visible through `CTX_PATH`, but it must refuse ordinary
visible tools and authority-bearing core tools such as `fs.write` and
`shell.exec`, because direct execution from `CTX_PATH` would skip the
agent/tool authorization stack.

When a terminal needs to be observable, its stable locator is the session
terminal socket:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

Because FUSE mounts generally cannot host a bound Unix socket directly, this
entry aliases `/run/cortexfs/terminal/broker.sock`. The bounded broker protocol
authenticates the peer and passes an accepted descriptor to `ctxterm`; raw PTY
bytes begin only after the offer/prepared/accepted/commit transaction. Per-user
terminal sockets and one-line mode prefixes are invalid. See
[terminal-broker.md](terminal-broker.md).

Human clients should use:

```text
ctx agent watch <agent> --session <session>
ctx agent attach <agent> --session <session>
```

`watch` is the safe default for observation. `attach` is an explicit writable
join and may affect the agent's terminal state.

Interactive shells and multiplexers are ordinary tools:

```text
/ctx/tool/bash
/ctx/tool/tmux
/ctx/tool/zellij
```

So an agent enters an interactive shell by asking `tsh` to run the `bash` tool.
Inside that tool, `exit` exits bash and returns to `tsh`. Background terminal
work should go through visible `tmux` or `zellij` tools, not through a second
agent scheduler namespace.

Example request:

```json
{
  "name": "reviewer",
  "label": "reviewer_t",
  "model": ["openai/gpt-5.6"],
  "tools": ["fs.read"],
  "shared": {
    "project-a": ["read"]
  },
  "mount": [
    ["/work", "/work", "ro"],
    ["/shared/project-a", "/shared/project-a", "ro"]
  ]
}
```

On success, the tool creates the ordinary `agent/<name>` executable entry,
socket entry when supported, and `agent/<name>.d/*` control files. It must not
create a new root namespace.

Agents search for tools by `CTX_PATH`:

```text
/ctx/tool/fs.read
/ctx/home/1000/tool/fs.read
/ctx/shared/project-a/tool/fs.read
```

For standalone human `tsh` sessions, the lookup path is chosen in this order:

```text
1. CTX_HOME/.tshrc line CTX_PATH=..., when present
2. process CTX_PATH, when set
3. default /ctx/tool:/ctx/home/<uid>/tool
```

`.tshrc` is not shell code. It is a user-level data file for persistent tool
path configuration, and it takes precedence over inherited process
environment.

When `tsh` runs inside an agent terminal, the agent runtime's process
`CTX_PATH` is authoritative because it is derived from policy, mounts, and
uid/gid.

`tsh` persistent runtime configuration lives in the `tsh` tool control
directory:

```text
/ctx/tool/tsh.d/config
```

The file is data, not shell code. It accepts blank lines, `#` comments, and
these `key=value` settings:

```text
max_loaded_tools=64
cache_capacity=32
window_percent=1
```

`max_loaded_tools` limits unpinned tool metadata entries loaded into the `tsh`
context. `cache_capacity` limits unpinned tool-path cache entries tracked by
W-TinyLFU. `window_percent` configures the W-TinyLFU admission window. Pinned
tools are excluded from both automatic context unload and path-cache eviction.
These settings do not imply that core loads SDK dynamic libraries.

The durable configuration should normally be updated through the visible tool:

```text
/ctx/tool/tsh.config
```

The search path above describes source tiers. The agent process may see a
filtered memory projection instead of the raw durable directories. The
projection must preserve object ABI shape and must not create durable files as
an authorization side effect.

stdin/stdout is the main tool interface. `schema` is the tool's input JSON
Schema. It does not grant permission and is not a claim about result shape.

## Programmatic Tool Calling

Programmatic tool calling is **not enabled** in the current ABI. CortexFS must
not advertise an OpenAI `programmatic_tool_calling` tool, set
`allowed_callers`, or treat `schema` as an `output_schema`. The existing
single-call host loop remains the only executable-agent tool protocol.

Before enablement, a tool needs an explicit, default-deny programmatic
declaration. The optional `tool/<name>.d/program` control is the tool author's
`readonly`/deterministic claim and one valid JSON Schema for a successful
result, for example:

```json
{
  "type": "object",
  "additionalProperties": false
}
```

The claim is explicit, not inferred from policy, name, or input schema. It
does not grant permission, bypass approval, or make a tool direct-native.
A missing, malformed, non-object, or schema-invalid control makes the tool
ineligible.

This control is accepted and validated by object install, layout inspection,
and bootstrap, but is reserved for the future protocol. No request builder may
consume it until the continuation and audit requirements below exist.

### Enablement Contract

Only a provider/model route which explicitly supports PTC may receive the
hosted `programmatic_tool_calling` tool. Its `allowed_callers` may name only
functions that pass all of these gates for this run:

- the route and selected model declare PTC support;
- `tool/<name>.d/program` is valid and its output Schema is bounded by the
  normal control limits;
- the tool is declared, resolves through `CTX_PATH`, and passes the normal
  effective-authority checks; and
- the effective run approval mode is `auto` (a tool that would require `Ask`
  is ineligible), and the tool has no side effect or external write.

The host must default to ordinary native tool calling when any gate is absent;
it must never infer eligibility from a tool name, policy, input Schema, or MCP
origin. The client-owned call interface remains host-owned: a generated
program has no process, socket, filesystem, or direct-native authority.

An enabled implementation must preserve the provider response's bounded,
opaque continuation facts: response identity, program item identity, each
nested `call_id`, and its exact opaque `caller` linkage. The durable audit
chain associates those facts with the host-owned request, authorization
decision, normalized tool result, and the continuation that consumed it. It
uses fields on existing host-owned facts, not a new CortexFS root namespace or
stable event type. Stateless/manual continuation retains the bounded request
and output history needed to replay the same provider continuation; provider
identifiers and caller values are not parsed as CortexFS authority.

Every generated nested call is serial and re-enters declared-name, `CTX_PATH`,
policy, Linux/mount, nofollow, sandbox, cancellation, and a defensive `Ask`
check for an unexpected policy transition before execution. That check cannot
make an otherwise ineligible tool PTC-eligible. It never executes directly.
The host validates the normalized result against the declared program output
Schema before returning the exact provider `call_id` and opaque `caller` in
`function_call_output`. Only then may it issue the matching bounded
`program_output` continuation.
`program_output` is not the final assistant response: the final assistant
message is separately parsed, authorized as ordinary model output, recorded,
and evaluated.

The following cases fail closed before a continuation or final success is
accepted:

| Condition | Required result |
| --- | --- |
| Unsupported model, missing/invalid program control, side-effecting tool, or approval-sensitive tool | Do not advertise it to PTC; use the ordinary host loop. |
| Malformed/oversize program item, `caller`, `call_id`, or continuation identity | Reject the provider turn; do not execute a tool. |
| Duplicate program or nested call id | Reject before authorization; do not execute twice. |
| Cancellation before, during, or after a nested call | Stop the prepared work, record cancellation, and send no further continuation. |
| Unexpected `Ask` requirement or `Ask` rejection, timeout, EOF, or malformed decision | Reject as PTC-ineligible, record the host-owned denial, and do not return a successful function result or continue the program. |
| Tool failure or output that fails the program Schema | Record the normalized failure; do not emit `program_output`. |
| Invalid `program_output` or invalid/final assistant message | Reject that artifact independently; neither artifact validates the other. |

These are pre-enable requirements, not a claim that the current runtime,
provider adapters, or audit store implement PTC.

Execution visibility and permission are decided by all of:

```text
Linux execute bit
agent uid/gid/groups
agent mount table and noexec flag
agent policy v0 allow
tool's own policy
```

There is no `agent/<name>.d/tool`. Tool authorization is policy v0. Without
`allow <agent_type> tool:<name> execute`, return `EACCES`.

An implementation may list only executable tools that pass this full effective
authority check for the agent. A durable system tool under `/ctx/tool` is a
candidate, not a grant. User-level visibility and CortexFS security context
both constrain the final agent-visible tool set.

MCP-originated tools use the same policy object class:

```text
allow coder_t tool:github.search_issues execute
allow coder_t tool:figma.get_file execute
```

Tool lookup is strictly `CTX_PATH`:

```text
search left to right for the first executable file with that name
the matching .d/ follows the directory that contained the executable
non-executable files are not hits
```

## Shared

`shared/<name>` is a Linux-permissioned shared space:

```text
/ctx/shared/
  project-a/
    tool/
    data/
```

Agent visibility depends on:

```text
agent uid/gid
agent label
mount file
shared directory permissions
policy v0
```

Do not design a collaboration DSL here. Higher-level agents can create ordinary
files under `shared/<project>`.

### Shared queue file protocol

A project queue is file-native state, not a daemon or workflow engine:

```text
queue/
  inbox/
  pending/
  lease/
  claimed/
  done/
  failed/
```

For a request name `J` ending in `*.req.json`, the durable states are:

```text
pending/J
claimed/J/J + lease/J/worker
done/J + done/J.result
failed/J + failed/J.result
```

Rules:

```text
publish   write a sibling temporary file, sync it, then atomically rename it to pending/J
claim     mkdir claimed/J is the exclusive arbitration point; the winner renames pending/J to claimed/J/J
lease     sync the claim move before creating and syncing lease/J/worker; execution starts only after both are durable
finish    sync a sibling temporary result, atomically rename it to J.result without replacement, then rename the request beside it
recover   an abandoned claim may return to pending only when claim and lease evidence both exist and no pending or terminal entry conflicts
conflict  never overwrite pending, claimed, lease, result, or terminal request evidence; incomplete pairs require explicit reconciliation
```

After each rename or directory removal, sync every changed parent directory.
Invalid names, symlinks, and non-regular request or lease files are not queue
jobs. No background watcher, polling service, or additional root path is part
of this protocol.

`shared/cortexfs-docs` is reserved for the system-maintained Markdown manual
bundle:

```text
/ctx/shared/cortexfs-docs/
  README.md
  man/
    ctx.agent.md
    ctx.tool.md
    ctx.model.md
    ctx.coreutils.md
    ctx.root-abi.md
    ctx.session.md
    ctx.provider.md
```

`/ctx/shared/cortexfs-docs/man/*.md` are documentation mirrors and should stay
aligned with `docs/spec/*.md` so live manuals do not keep stale references.
When an installed `/ctx/shared/cortexfs-docs` tree is stale, run
`ctx bootstrap [SOURCE]` to rematerialize from the matching release source tree.
If stale content remains, the installed `ctx` binary is still shipping an older
embedded manual bundle and must be updated before rerunning bootstrap.

`ctx man TOPIC` prints these files directly when present and falls back to the
compiled-in copy when they are absent. Topic names such as `agent` and `model`
are CLI aliases; durable file names use the `ctx.*.md` namespace. The manual
bundle is ordinary read-only documentation data for users and agents. It does
not grant authority and must not become a second root ABI namespace.

Shared sessions are ordinary directories:

```text
/ctx/shared/project-a/
  agent/
    coder/
      session/
        design-review/
```

An agent can see or write these sessions only when Linux permissions, mount
visibility, and policy v0 all allow it. CortexFS does not provide
`agent/<name>.d/shared`.

## Policy v0

Permissions go from coarse to fine:

```text
Linux uid/gid
file mode bit
chroot + bind mount
agent label
tool/model/agent policy
```

Policy v0 is a minimal type-enforcement allowlist. It borrows SELinux subject
type, object class, permission, and default deny. It does not copy the full
SELinux policy language.

Format:

```text
allow <subject_type> <object_class>:<object_name> <permission>
```

Examples:

```text
allow coder_t tool:fs.read execute
allow coder_t tool:shell.exec execute
allow coder_t model:openai/gpt-5.6 use
allow coder_t shared:project-a read
allow coder_t shared:project-a write
allow coder_t network:default connect
allow coder_t agent:reviewer create
allow coder_t agent:reviewer start
```

Rules:

```text
default deny
no explicit deny
no glob
no priority
no inheritance
no variable expansion
no path matching
unknown class returns EINVAL
unknown permission returns EINVAL
missing object returns ENOENT or EACCES
```

Fixed permission set:

```text
tool:    execute
model:   use
shared:  read write
session: read write resume
mount:   read write
agent:   create start stop read write
network: connect
```

Agent policy uses concrete names:

```text
allow coder_t agent:reviewer create
allow coder_t agent:reviewer start
```

Do not add glob, inheritance, variables, or templates:

```text
allow coder_t agent:* create
```

The only stable network object name is `default`:

```text
allow coder_t network:default connect
```

Without `allow coder_t network:default connect`, there is no network access.

Permission check order:

```text
1. peer credential or exec uid/gid
2. Linux mode bit
3. mount/chroot visibility
4. agent label
5. object policy
6. tool/model policy
```

Any refusal refuses. Agent prompt, system prompt, and model output cannot grant
permission.

## Logs and Events

There is no root-level `audit/`. Logs and events live near the object:

```text
model/<provider>/<model>.d/log
agent/<name>.d/log
tool/<name>.d/log
home/<uid>/agent/<agent>/session/<session>/events.jsonl
shared/<name>/agent/<agent>/session/<session>/events.jsonl
```

Minimum event shape:

```json
{"ts":"2026-06-22T12:00:00Z","type":"tool.call","agent":"coder","session":"default","object":"tool/fs.read","status":"ok"}
```

Policy decides whether sensitive content is logged. Default logging should
record facts and errors, not full secrets or large prompt bodies.
