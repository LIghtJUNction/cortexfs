# Path ABI

`cortexfs-paths` is the public path ABI for CortexFS integrations. The main
workspace and this crate use the same release version. Applications that need
to locate CortexFS files or sockets depend on this crate instead of copying
implementation literals.

## Stable roots

The public FUSE root is `/ctx`. Its only stable top-level entries are:

```text
 status  bin  model  agent  tool  channel  home  shared
```

The crate exports `CTX_ROOT`, `ROOT_ENTRIES`, and compositors for each root,
object, control, model, agent, tool, home, shared, durable-session, and
attach-channel path. It also composes global and per-user channel instance,
control, and tool paths.
Unknown root entries are not accepted by `root_entry_path`.

## Agent socket roles

One agent may have three different socket paths. They must not be conflated:

| Role | Crate function | Example |
| --- | --- | --- |
| public client ABI | `agent_client_socket` | `/ctx/agent/coder.sock` |
| private system runtime | `system_agent_runtime_socket` | `/run/cortexfs/agent/coder.sock` |
| durable backing tree | `agent_backing_socket` | `/var/lib/cortexfs/storage/current/agent/coder.sock` |

The public client path is the channel adapter contract unless a deployment
explicitly configures another runtime contract. A channel adapter must not
derive a second socket location from the host filesystem.

Terminal resources likewise distinguish the durable `/ctx/home/.../terminal`
path from the live `/run/user/<uid>/cortexfs/...` transport path.

## Host paths

The crate owns the stable host locations for:

- durable storage and its selected `current` generation;
- provider configuration, provider secrets, and model cache;
- file-configured channel adapters under `/etc/cortexfs/channels`;
- system agent runtime and control sockets.

These functions only compose `PathBuf` values. They do not create files,
follow links, start services, or grant authority. Callers retain the existing
plain-file checks, permissions, atomic temporary-file-plus-rename rule, and
audit append semantics.

## Dynamic components

`validate_component` rejects empty, dot-like, separator-containing, NUL, and
overlong values. It is a general path-component guard; the main implementation
may apply stricter object-name rules where the object ABI requires them.

The crate has no runtime dependencies and is safe to use from channel hosts,
SDK extensions, agent runtimes, and administration tools.

Attach discovery uses the agent's durable session collection (the directory
containing `index/`), not an individual session directory:

```rust
use cortexfs_paths::{agent_sessions_path, ctx_root, session_channel_index_path, session_channel_path};

let sessions = agent_sessions_path(&ctx_root(), "1000", "coder");
let channels = session_channel_index_path(&sessions);
let terminal = session_channel_path(&sessions, "terminal_coder_default");
```
