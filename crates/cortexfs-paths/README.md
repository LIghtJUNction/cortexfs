# cortexfs-paths

`cortexfs-paths` is the stable path ABI for CortexFS integrations. It keeps
the public `/ctx` tree, system runtime sockets, durable storage, and host
configuration paths in one small dependency so applications do not copy
path literals from the CortexFS implementation.

```toml
cortexfs-paths = "0.1.17"
```

The crate is intentionally dependency-free. It composes paths and exposes the
canonical constants; callers still validate user-controlled object names with
`validate_component` before using them in a path.

The API separates the three important agent socket roles:

```rust
use cortexfs_paths::{agent_backing_socket, agent_client_socket, system_agent_runtime_socket};
use std::path::Path;

assert_eq!(agent_client_socket("coder"), Path::new("/ctx/agent/coder.sock"));
assert_eq!(
    system_agent_runtime_socket("coder"),
    Path::new("/run/cortexfs/agent/coder.sock")
);
assert_eq!(
    agent_backing_socket(Path::new("/var/lib/cortexfs/storage/current"), "coder"),
    Path::new("/var/lib/cortexfs/storage/current/agent/coder.sock")
);
```

The public client endpoint for a system agent is
`agent_client_socket("coder")`, which returns `/ctx/agent/coder.sock`.
`system_agent_runtime_socket("coder")` returns the private systemd listener
path `/run/cortexfs/agent/coder.sock`; it is lifecycle data, not a user or
channel configuration value.

Attachable terminal, web, and IM frontends are ordinary files below the
existing durable session index. Use the public compositors instead of
copying `index/channel` literals:

```rust
use cortexfs_paths::{session_channel_index_path, session_channel_path};

let sessions = Path::new("/ctx/home/1000/agent/coder/session");
let channels = session_channel_index_path(sessions);
let terminal = session_channel_path(sessions, "terminal_coder_default");
```

Channel paths use the same ABI crate:

```rust
use cortexfs_paths::{channel_tool_path, home_channel_control_file_path};
let global_tools = channel_tool_path(Path::new("/ctx"), "discord");
let user_cap = home_channel_control_file_path(Path::new("/ctx"), "1000", "discord", "cap");
```

There is no `channel/tool` compositor. Common tools live below each named
channel, and the runtime resolves user paths before global paths.
