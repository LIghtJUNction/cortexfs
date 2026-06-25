# Tool, Shared, Policy, and Logs ABI

This file continues [agent-tool-security.md](agent-tool-security.md). It keeps
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
  fs.write
  fs.write.d/
    name
    description
    schema
    cap
    policy
    status
    log
  shell.exec
  shell.exec.d/
    name
    description
    schema
    cap
    policy
    status
    log
```

MCP servers are tool sources, not CortexFS root objects. Do not expose:

```text
/ctx/mcp/github
/ctx/mcp/figma
```

After a real MCP adapter is configured, expose its capabilities as ordinary
tools:

```text
/ctx/tool/mcp.github.search_issues
/ctx/tool/mcp.github.create_issue
/ctx/tool/mcp.figma.get_file
/ctx/tool/mcp.chrome.open
```

MCP-backed capabilities may be projected as ordinary tools, but they are not
default built-ins. CortexFS does not define where MCP servers are configured.
The agent runtime or tool adapter may discover MCP servers from ordinary files
visible inside the agent view.

Projected tool control files remain the ordinary tool ABI:

```text
/ctx/tool/mcp.github.search_issues.d/schema
/ctx/tool/mcp.github.search_issues.d/policy
/ctx/tool/mcp.github.search_issues.d/status
/ctx/tool/mcp.github.search_issues.d/log
```

An implementation may expose an optional diagnostic origin file:

```text
/ctx/tool/mcp.github.search_issues.d/origin
```

`origin` is not stable ABI. Strict clients must not depend on it. MCP is only
where the tool came from; it is not a new namespace, policy class, submission
path, or CortexFS-defined server configuration format.

Tools are executable capability endpoints:

```bash
/ctx/tool/fs.read '{"path":"README.md"}'
echo '{"cmd":"pwd"}' | /ctx/tool/shell.exec
```

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
output. The native agent mode may load the same tool into memory and call it
through the SDK ABI, using structured JSON input and JSONL tool frames. `load`
and `pin` affect the native context/cache; they must not force terminal CLI
commands to emit structured frames.

When a terminal needs to be observable, `ctxterm` listens on the session
terminal socket:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

Because FUSE mounts generally cannot host a bound Unix socket directly, this
visible ABI path may be a symlink to a runtime socket under `/run`, for example:

```text
/run/cortexfs/terminal/<uid>/<agent>/<session>/main.sock
```

The socket protocol is raw PTY bytes after a one-line client mode:

```text
watch\n   read PTY output only
attach\n  read PTY output and write stdin to the PTY
```

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
  "model": ["openai/gpt-4o"],
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

For human `tsh` sessions, the lookup path is chosen in this order:

```text
1. process CTX_PATH, when set
2. CTX_HOME/.tshrc line CTX_PATH=...
3. default /ctx/tool:/ctx/home/<uid>/tool
```

`.tshrc` is not shell code. It is a user-level data file for persistent tool
path configuration.

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
context. `cache_capacity` limits unpinned dynamic tool artifacts kept resident
in memory by W-TinyLFU. `window_percent` configures the W-TinyLFU admission
window. Pinned tools are excluded from both automatic context unload and dynamic
cache eviction.

The durable configuration should normally be updated through the visible tool:

```text
/ctx/tool/tsh.config
```

The search path above describes source tiers. The agent process may see a
filtered memory projection instead of the raw durable directories. The
projection must preserve object ABI shape and must not create durable files as
an authorization side effect.

stdin/stdout is the main tool interface. `schema` describes input and output.
It does not grant permission.

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
allow coder_t tool:mcp.github.search_issues execute
allow coder_t tool:mcp.figma.get_file execute
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
allow coder_t model:openai/gpt-4o use
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

Fixed v1 permission set:

```text
tool:    execute
model:   use
shared:  read write
session: read write resume
mount:   read write
agent:   create start stop read write
network: connect
```

Agent policy uses concrete names in v1:

```text
allow coder_t agent:reviewer create
allow coder_t agent:reviewer start
```

Do not add glob, inheritance, variables, or templates:

```text
allow coder_t agent:* create
```

The only v1 network object name is `default`:

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
