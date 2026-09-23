# CortexFS Specification

This directory is the normative CortexFS ABI specification.

CortexFS exposes a small Linux filesystem and execution boundary to existing
Agent CLIs. Paths are ABI. Executable things are files. Control state lives
next to the object in `<name>.d/`. Stateful interaction uses `<name>.sock`.

The stable shape is:

```text
/ctx/
  status
  bin/
  model/
  agent/
  tool/
  channel/
  home/
  shared/
```

Core principles:

```text
root is frozen
root contains stable object classes only
/ctx is read-write; individual paths may be read-only by Unix/FUSE policy
Agent CLIs own model/provider auth, session/context, approvals, and tool loops
CortexFS owns process, path, mount, identity, policy, and FUSE boundaries
agent objects describe principals and executable boundaries, not a second AI runtime
tools are executable capability endpoints
prompt, skill, and project-rule text never grants authority
provider/API format details do not enter root ABI
backing storage must never be exposed as a writable bind that bypasses FUSE
```

Execution boundary:

```text
one backend-neutral process contract: executable/argv, cwd, env, stdio or PTY,
exit status, cancellation/signals, bounded resources, and authorized mounts
backend-specific spelling stays in thin launch profiles/adapters
Codex CLI, Claude Code, and Pi are first-class hosted CLIs
Antigravity CLI is the current Google-side extension target
MCP and other open protocols are reused directly instead of wrapped in a new wire protocol
```

Compatibility boundary:

```text
legacy CortexFS model/tool/session runtime behavior remains only where compatibility requires it
new work must not grow another provider/model/tool-loop/session authority beside hosted CLIs
legacy runtime pieces are narrowed or retired behind explicit migrations
stable root/path ABI remains compatible unless a migration spec says otherwise
```

CortexFS does not define these as root ABI:

```text
provider registry
API format registry
database backend
vector database backend
workflow/job/hook DSL
spawn/factory/agent-template root
cluster scheduler DSL
MCP registry root; MCP servers are external config and may project ordinary tools
skill registry root; skill files are ordinary visible files and grant no authority
audit root
control root
```

Spec files:

```text
root-abi.md             frozen /ctx root, stable reference tree, and basic file rules
fuse.md                 FUSE projection shape
object-abi.md           executable, socket, and .d object triple
model-abi.md            compatibility model object ABI and event stream
session-abi.md          durable CortexFS session facts and indexes
agent-tool-security.md  agent identity, view, mount, and creation
agent-runtime.md        legacy hosted-runtime compatibility and migration boundary
module-abi.md           static module API and stable external wire contract
terminal-abi.md         durable terminal resources, PTY events, and attach
terminal-broker.md      root broker authentication and descriptor grants
tool-policy-abi.md      tool ABI, MCP projection, shared, policy, logs
ctx-coreutils.md        ctx command contract
rolling-upgrades.md     rolling reference-tree update and storage switch rules
channel-abi.md          multi-IM channel crate, routing, and host boundary
interaction-abi.md      frontend/runtime bidirectional interaction frames
paths.md                public path constants and filesystem/socket path ABI
```

`agent-runtime.md` still documents compatibility behavior that exists in the
current implementation. It is not the target architecture for new Agent
features. Migration toward hosted Agent CLIs is tracked in issue #318; changes
to the legacy runtime should reduce or preserve its surface, not extend it.

## External references

- CortexFS normative docs and implementation in this repository.
- [Model Context Protocol](https://modelcontextprotocol.io/specification/)
- [Linux FUSE project docs](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)
- [mcp-filesystem implementations](https://github.com/search?q=mcp-filesystem)
