# Agent, Tool, and Security ABI

Layer boundary:

```text
model = pure inference endpoint
tool  = executable capability endpoint
agent = policy-bound orchestrator process
```

By default, a model has no:

```text
tool permission
filesystem write permission
project context
long-term memory
task planning
chroot/mount policy
MCP/skill
cluster scheduling
```

An agent owns Linux uid/gid/groups, label, home, root, cwd, mounts, policy,
context, and tool execution decisions. Real file writes, tool calls, and
shared-space access are attributed to the agent, not to the model.

Tool boundary:

```text
model may emit tool_call events
model must not execute tools
agent decides whether to execute tools
agent policy decides whether execution is allowed
```

## Agent as File

```text
/ctx/agent/
  coder
  coder.sock
  coder.d/
    owner
    uid
    gid
    groups
    label
    iso
    parent
    life
    root
    cwd
    env
    path
    mount
    model
    policy
    status
    pid
    log
    meta.json
```

Control files:

```text
owner   owning Linux user uid
uid     runtime uid; defaults to owner
gid     runtime gid
groups  supplementary groups, one gid per line
label   CortexFS agent label, for example user_u:agent_r:coder_t:s0
iso     isolation profile: shared, uid, or userns
parent  parent agent, session, or run that created this agent
life    lifecycle ownership, default owned
```

Multiple agents may share one Linux uid. The uid expresses the user boundary.
The label expresses the agent security boundary.

`meta.json` may exist for longer descriptions such as purpose, creation time,
or issue number. Policy decisions must not depend on `meta.json`.

Agent startup:

```text
1. Read /ctx/agent/<name>.d/*
2. Set CTX_ROOT, CTX_HOME, and CTX_PATH
3. Merge agent/<name>.d/env
4. Establish runtime identity from uid/gid/groups/label
5. Create the mount namespace
6. Apply bind mounts from mount
7. chroot to root
8. cd to cwd
9. exec the agent runtime
```

## Agent View

An agent view is the set of files, tools, models, sockets, and shared spaces
visible to an agent.

It is derived from:

```text
root
cwd
mount
path
model
policy
Linux uid/gid/groups/mode bits
CortexFS label
```

CortexFS does not define MCP config formats, skill formats, project rule
formats, or prompt package formats. Those are ordinary files.

Examples:

```text
/home/alex/.codex/config.toml  /home/agent/.codex/config.toml  ro  bind,nosuid,nodev,noexec
/home/alex/project/.mcp.json   /work/.mcp.json                 ro  bind,nosuid,nodev,noexec
```

An agent may read those files only if they are visible inside its chroot or
mount namespace and allowed by Linux permissions. Executing any capability
derived from them still requires CortexFS tool policy.

Skill files are ordinary files visible through the agent mount namespace.
CortexFS does not define skill file formats. Skill visibility is determined by
mount visibility, Linux permissions, and policy. Skills do not grant authority.

Lifecycle:

```text
start
ready
busy
idle
stopping
dead
```

v1 does not introduce a global daemon. Prefer each agent process owning its own
socket, pid, log, and sessions. A future supervisor is implementation detail.
It must not add another root directory.

## Agent Home

An agent does not use the user's home directly:

```text
/ctx/home/1000/
  agent/
    coder/
      root/
      session/
      data/
      cache/
      log/
  tool/
  model/
```

Recommended config:

```text
/ctx/agent/coder.d/root = /ctx/home/1000/agent/coder/root
/ctx/agent/coder.d/cwd  = /work
```

Runtime environment:

```sh
CTX_ROOT=/ctx
CTX_HOME=/ctx/home/1000
HOME=/ctx/home/1000/agent/coder
CTX_PATH=/ctx/tool:/ctx/home/1000/tool
```

## Mount File

`/ctx/agent/<name>.d/mount` format:

```text
source<TAB>target<TAB>mode<TAB>options
```

v0 parser rules:

```text
source and target must be absolute paths
source and target must not contain TAB or newline; return EINVAL if they do
mode is only ro or rw
options is a comma-separated list of small words
unknown option returns EINVAL
```

Fixed v0 option set:

```text
bind
rbind
nosuid
nodev
noexec
-
```

`-` means no extra option. Except for `-`, options must not repeat. `bind` and
`rbind` are mutually exclusive.

Example:

```text
/ctx	/ctx	ro	rbind,nosuid,nodev
/ctx/home/1000/agent/coder	/home/agent	rw	rbind,nosuid,nodev
/home/me/project	/work	rw	rbind,nosuid,nodev
/ctx/shared/project-a	/shared/project-a	rw	rbind,nosuid,nodev
/tmp	/tmp	rw	rbind,nosuid,nodev
```

## Agent Creation

An agent can create another agent only through normal CortexFS objects and
policy checks. There is no root-level `spawn/`, `factory/`, or
`agent-template/`.

The child appears as ordinary agent ABI:

```text
/ctx/agent/reviewer
/ctx/agent/reviewer.sock
/ctx/agent/reviewer.d/
  owner
  uid
  gid
  groups
  label
  iso
  parent
  life
  root
  cwd
  env
  path
  mount
  model
  policy
  status
  pid
  log
```

`parent` is a small text file. v1 should keep it simple:

```text
agent:coder
```

or, when needed:

```text
agent:coder session:default run:01H...
```

Do not turn lineage into a separate tree in v1.

Child defaults:

```text
owner  = parent owner
uid    = parent uid
gid    = parent gid
groups = subset of parent groups
iso    = shared
life   = owned
```

Every agent should have a distinct CortexFS label unless it is intentionally
the same security domain. A child that reuses the parent's label is the same
security subject for policy purposes.

Child agent attenuation is mandatory:

```text
child permissions must be a subset of parent effective permissions
child policy must be a subset of parent effective policy unless a supervisor grants more
child groups must be a subset of parent groups unless a supervisor grants more
child mounts must be derived from parent-visible mounts
```

Mount attenuation:

```text
parent rw may become child ro
parent visible may become child hidden
parent ro must not become child rw
parent hidden must not become child visible
```

A child mount must not expose paths invisible to the parent. For example, a
parent that sees `/work` and `/shared/project-a` may grant the child read-only
views of those paths, but not `/home/user`, `/etc`, `/var/log`, or
`/shared/project-b` unless a supervisor authorizes them.

Owned child agents are cancelled when the parent dies. Parent death cancels
the child runtime, not the child's session history. See `17-child-agents.md`
for handoff, result, and lifecycle rules.

Names should stay short:

```text
coder
reviewer
planner
runner
worker1
fix-123
rev-123
```

Put longer descriptions in `agent/<name>.d/meta.json`.

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

Expose MCP capabilities as ordinary tools:

```text
/ctx/tool/mcp.github.search_issues
/ctx/tool/mcp.github.create_issue
/ctx/tool/mcp.figma.get_file
/ctx/tool/mcp.chrome.open
```

MCP-backed capabilities may be projected as ordinary tools. CortexFS does not
define where MCP servers are configured. The agent runtime or tool adapter may
discover MCP servers from ordinary files visible inside the agent view.

Tool control files remain the ordinary tool ABI:

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

Optional standard agent lifecycle tools:

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

Example request:

```json
{
  "name": "reviewer",
  "label": "reviewer_t",
  "model": ["qwen"],
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

stdin/stdout is the main tool interface. `schema` describes input and output.
It does not grant permission.

Execution permission is decided by all four:

```text
Linux execute bit
agent uid/gid
agent policy v0 allow
tool's own policy
```

There is no `agent/<name>.d/tool`. Tool authorization is policy v0. Without
`allow <agent_type> tool:<name> execute`, return `EACCES`.

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
allow coder_t model:qwen use
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
model/<name>.d/log
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
