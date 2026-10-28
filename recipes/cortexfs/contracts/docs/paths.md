---
sidebar_position: 6
---

# Path ABI and cortexfs-paths

cortexfs-paths is the single path-layout crate for CortexFS integrations. It
is published independently so channel hosts, agent runtimes, SDK extensions,
and administration tools can derive paths without copying literals from the
main implementation:

~~~toml
cortexfs-paths = "0.1.21"
~~~

Its version follows the workspace version of CortexFS. A CortexFS release
publishes the main packages and cortexfs-paths with the same version number.
The crate has no runtime dependencies and only composes paths; it does not
create directories, open files, mount FUSE, or start a daemon.

## Three path roles

The same agent can have several paths. They are intentionally different:

| Role | API | Example |
| --- | --- | --- |
| Public client ABI | agent_client_socket | /ctx/agent/executor.sock |
| Private systemd listener | system_agent_runtime_socket | /run/cortexfs/agent/executor.sock |
| Durable backing tree | agent_backing_socket | /var/lib/cortexfs/storage/current/agent/executor.sock |
| User terminal ABI | session_terminal_path | /ctx/home/1000/agent/executor/session/default/terminal/main.sock |
| Attach channel index | session_channel_index_path | /ctx/home/1000/agent/executor/session/index/channel |
| Attach channel file | session_channel_path | /ctx/home/1000/agent/executor/session/index/channel/terminal_executor_default |
| Terminal broker endpoint | BROKER_SOCKET | /run/cortexfs/terminal/broker.sock |

An IM adapter normally reads its configured agent_socket from
agent_client_socket or from the explicitly configured runtime contract. It
must not guess a second socket location. The Discord host configuration file
itself is channel_config_path("discord"), which resolves to
/etc/cortexfs/channels/discord.toml.

## Public /ctx layout

The frozen top-level entries are exported as ROOT_ENTRIES:

~~~text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
~~~

Use the role-specific functions for object and session paths:

~~~rust
use cortexfs_paths::{
    agent_control_file_path, agent_socket_path, ctx_root, model_path,
    session_file_path, tool_path, validate_component,
};

let root = ctx_root();
validate_component("executor")?;
let socket = agent_socket_path(&root, "executor");
let status = agent_control_file_path(&root, "executor", "status");
let model = model_path(&root, "openai", "gpt-5.6");
let tool = tool_path(&root, "fs.read");
let messages = session_file_path(&root, "1000", "executor", "default", "messages.jsonl");
~~~

Dynamic names are deliberately accepted as &str so the crate stays
dependency-free. Call validate_component (or apply the host application's
equivalent object-name policy) before composing untrusted values. The
composition helpers do not access the filesystem and do not follow symlinks.

## Host paths

The crate also centralizes paths outside /ctx:

- SYSTEM_STORAGE_DIR and storage_root_path() identify durable storage.
- SYSTEM_STORAGE_CURRENT and storage_current_path() identify the selected
  generation.
- provider_config_path, provider_secret_path, and
  provider_model_cache_path identify provider state without exposing secrets
  through /ctx.
- channel_config_path identifies file-configured IM adapters.
- SYSTEM_RUNTIME_DIR, RUN_CONTROL_SOCKET, and the agent runtime helpers
  identify the private runtime plane.

Callers own permissions and atomic write semantics. Configuration changes still
follow the CortexFS rule: write a sibling temporary file, sync it, atomically
rename the final file, and append the resulting fact to the appropriate audit
stream.

## Compatibility rule

cortexfs-paths is the ABI boundary for path names. New integrations should
depend on it directly:

~~~toml
cortexfs-paths = "0.1.21"
~~~

Do not duplicate /ctx, /run/cortexfs, /var/lib/cortexfs, or /etc/cortexfs
literals in an adapter. When the path ABI changes, the crate version changes
with the main project and the migration is documented here and in the
normative docs/spec/ files.
