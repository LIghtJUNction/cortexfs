# ABI specification archive

[Documentation archive](../README.md) · [简体中文目录](../translations/zh-Hans/spec/README.md) · [Current scope](../../PROJECT_SCOPE.md)

> **Retired contract.** These documents describe the former CortexFS FUSE /ctx and Agent runtime design retained through source snapshot 7261cb3. They are historical references, not active interfaces or compatibility requirements. The earlier 84f349c reconstruction snapshot has a separate bundled specification under the historical recipe.

## What the old ABI described

The former ABI specified paths, file operations, permissions, executable objects, control directories, sockets, and message formats. Its /ctx root contained eight entries: status, bin, model, agent, tool, channel, home, and shared.

The old design assigned model authentication, conversation state, and tool loops to hosted CLIs, while CortexFS owned process/resource boundaries and FUSE projection. None of these responsibilities is an approved requirement for the next product.

## Archived contracts

| Historical document | Former subject |
| --- | --- |
| [Root ABI](root-abi.md) | Eight root entries and basic file rules |
| [Paths](paths.md) | Filesystem/socket names and host path roles |
| [FUSE](fuse.md) | Filesystem projection and operations |
| [Objects](object-abi.md) | Executable, socket, and control-directory interface |
| [Models](model-abi.md) | Model objects and inference events |
| [Sessions](session-abi.md) | Durable session facts and indexes |
| [Agent security](agent-tool-security.md) | Identity, visibility, mounts, and creation |
| [Agent runtime](agent-runtime.md) | Legacy runtime behavior |
| [Tools and policy](tool-policy-abi.md) | Tools, MCP projection, permissions, shared state, and audit |
| [Terminals](terminal-abi.md), [Terminal broker](terminal-broker.md) | PTY events, attachment, and descriptor grants |
| [Interaction](interaction-abi.md) | Frontend/runtime frames |
| [Channels](channel-abi.md) | Channel instances and routing |
| [Modules](module-abi.md) | Module API and external wire contract |
| [ctx coreutils](ctx-coreutils.md) | Old command behavior and output |
| [Rolling upgrades](rolling-upgrades.md) | Reference-tree and storage-generation updates |

Do not extend these interfaces to accommodate a hypothetical product. If a future task genuinely needs a protocol, write a new minimal contract for that task.

## Historical references

- [MCP specification](https://modelcontextprotocol.io/specification/)
- [Linux FUSE documentation](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)