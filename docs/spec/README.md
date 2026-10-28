# CortexFS Specification

This directory is the normative CortexFS ABI specification. It describes the
`v0.1.21` source snapshot recorded in [`../README.md`](../README.md); the
implementation has been removed from the current repository tree.

CortexFS exposes agent runtime behavior through a small Linux filesystem
interface. Paths, object execution, durable session facts, permissions, and
versioned socket frames are externally observable contracts. Provider, storage,
and implementation choices remain free when they do not affect those
contracts.

The stable root contains exactly these eight entries:

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

Core boundaries:

```text
model emits inference events; it does not execute tools
agent decides whether to request a tool; policy decides whether it may run
tool execution is an explicit capability endpoint
session history is durable; prompt context is rebuildable
provider/API details do not create new root namespaces
all submissions use the documented atomic request, outbox, and audit semantics
```

## Normative files

| File | Contract |
| --- | --- |
| [root-abi.md](root-abi.md) | Stable `/ctx` root, reference tree, and file rules |
| [paths.md](paths.md) | Public path names and host path roles |
| [fuse.md](fuse.md) | FUSE projection and filesystem behavior |
| [object-abi.md](object-abi.md) | Executable, socket, and `.d/` object surfaces |
| [model-abi.md](model-abi.md) | Model execution, routing, authentication, and event stream |
| [session-abi.md](session-abi.md) | Durable history, run identity, and session indexes |
| [agent-tool-security.md](agent-tool-security.md) | Agent identity, visibility, mounts, and authority |
| [agent-runtime.md](agent-runtime.md) | Runtime, context, tools, child work, and sandbox behavior |
| [tool-policy-abi.md](tool-policy-abi.md) | Tool files, policy, shared data, and audit facts |
| [terminal-abi.md](terminal-abi.md) | Terminal resources, PTY events, and attachment |
| [terminal-broker.md](terminal-broker.md) | Terminal broker authentication and descriptor grants |
| [interaction-abi.md](interaction-abi.md) | Frontend/runtime interaction frames and compatibility |
| [channel-abi.md](channel-abi.md) | Channel instances, routing, drivers, and host boundary |
| [module-abi.md](module-abi.md) | External module metadata, lifecycle, and wire format |
| [ctx-coreutils.md](ctx-coreutils.md) | `ctx` command and output contract |
| [rolling-upgrades.md](rolling-upgrades.md) | Reference-tree updates and storage-generation switching |

## External references

- [Model Context Protocol](https://modelcontextprotocol.io/specification/)
- [Linux FUSE documentation](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)
- [Linux `mount.fuse` manual](https://manpages.ubuntu.com/manpages/jammy/man8/mount.fuse.8.html)
