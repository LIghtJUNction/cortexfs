# CortexFS Specification

This directory is the normative CortexFS ABI specification.

CortexFS turns an AI runtime into a small Linux filesystem interface. Paths are
ABI. Executable things are files. Control state lives next to the object in
`<name>.d/`. Stateful interaction uses `<name>.sock`.

The stable shape is:

```text
/ctx/
  status
  bin/
  model/
  agent/
  tool/
  home/
  shared/
```

Core principles:

```text
root is frozen
root contains stable object classes only
models are pure inference endpoints
agents own orchestration and permission
tools are executable capability endpoints
sessions are ordinary files
context is a rebuildable working set
raw history is durable
independent tasks should run in child agents
owned child agents die when their parent dies
sockets speak JSONL
control files are small text files
provider/API format details do not enter root ABI
```

Tool boundary:

```text
model may emit tool_call events
model must not execute tools
agent decides whether to execute tools
agent policy decides whether execution is allowed
```

Protocol boundary:

```text
CortexFS owns: file ABI, agent lifecycle, socket sessions, permissions, chroot, bind mounts, CTX_PATH, shared/home
protocol adapters own: provider connections, API format compatibility, model calls, stream/event adaptation, low-level provider quirks
agent         owns: tool loops, context shaping, child task handoff, whether to execute tools
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
model-abi.md            one model ABI, model exec, model socket, event stream
session-abi.md          durable history and session indexes
agent-tool-security.md  agent identity, view, mount, and creation
agent-runtime.md        end-to-end agent runtime, REPL, terminal, tsh, sandbox
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

## External references

- CortexFS normative docs and implementation in this repository.
- [Model Context Protocol](https://modelcontextprotocol.io/specification/)
- [Linux FUSE project docs](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)
- [mcp-filesystem implementations](https://github.com/search?q=mcp-filesystem)
