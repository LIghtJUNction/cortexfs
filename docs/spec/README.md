# ABI specification

[Documentation](../README.md) · [简体中文目录](../translations/zh-Hans/spec/README.md) · [Architecture](../architecture.md)

These contracts are retained through source snapshot `7261cb3`. Product
implementation code is absent from this checkout. To reconstruct the earlier
`84f349c` baseline, use the recipe's [bundled specification](../../recipes/cortexfs/contracts/docs/spec/README.md).
The [version guide](../README.md#versions-and-authority) identifies both sources.

## Read the interface

An ABI is the observable contract between a client and the runtime: paths,
file operations, permissions, and message formats. CortexFS exposes executable
objects as `<name>`, their controls as `<name>.d/`, and stateful interaction as
`<name>.sock`. Start with [Root ABI](root-abi.md) and [Object ABI](object-abi.md).

The `/ctx` root contains exactly eight entries: `status`, `bin/`, `model/`,
`agent/`, `tool/`, `channel/`, `home/`, and `shared/`. New provider names,
protocols, and internal storage choices belong within existing interfaces.

Three rules connect the contracts:

- Unix identity, permissions, mount visibility, and CortexFS policy bound
  authority. Prompt, skill, and project-rule text cannot expand it.
- `/ctx` is a read-write FUSE filesystem with per-path restrictions. Writable
  backing-store binds must never bypass its enforcement.
- The hosted CLI design assigns model authentication, conversation state, and
  the tool loop to the CLI. CortexFS owns the process and resource boundary;
  the [launch contract](../internal-architecture.md) defines the details.

## Find a contract

| Document | Contract |
| --- | --- |
| [Root ABI](root-abi.md) | Eight root entries, reference tree, and basic file rules |
| [Paths](paths.md) | Public filesystem/socket names and host path roles |
| [FUSE](fuse.md) | Filesystem projection and operations |
| [Objects](object-abi.md) | Executable, socket, and control-directory interface |
| [Models](model-abi.md) | Compatibility model objects and inference events |
| [Sessions](session-abi.md) | Durable session facts and indexes |
| [Agent security](agent-tool-security.md) | Identity, visibility, mounts, and creation |
| [Agent runtime](agent-runtime.md) | Legacy runtime behavior and migration constraints |
| [Tools and policy](tool-policy-abi.md) | Tools, MCP projection, permissions, shared state, and audit |
| [Terminals](terminal-abi.md) | Terminal resources, PTY events, and attachment |
| [Terminal broker](terminal-broker.md) | Broker authentication and descriptor grants |
| [Interaction](interaction-abi.md) | Frontend/runtime frames and version compatibility |
| [Channels](channel-abi.md) | Channel instances, routing, drivers, and host boundary |
| [Modules](module-abi.md) | Module API and external wire contract |
| [`ctx` coreutils](ctx-coreutils.md) | Command behavior and output |
| [Rolling upgrades](rolling-upgrades.md) | Reference-tree updates and storage-generation switching |

## Compatibility and historical conflicts

The retained documents span the migration toward hosted Agent CLIs. Legacy
model, tool, and session contracts record compatibility obligations. Target
integrations and migration milestones are described in
[Architecture](../architecture.md); they are not current checkout test results.

One known conflict matters when reading [Agent runtime](agent-runtime.md): its
implicit read-only `.git` over-mount was removed in the later source snapshot
by [#320](https://github.com/LIghtJUNction/cortexfs/pull/320). An authorized
read-write workspace follows ordinary Unix subtree permissions. Explicit
policy can narrow `.git`, and linked-worktree metadata outside that subtree
still needs separate authorization.

Detailed English contracts take precedence over summaries and older
translations. Keep identified conflicts explicit and keep the recipe's fixed
snapshot separate from later migration decisions.

## External references

- [Model Context Protocol specification](https://modelcontextprotocol.io/specification/)
- [Linux FUSE documentation](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)
