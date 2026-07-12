# Object ABI

Every executable object follows the same triple:

```text
name        executable entry: stateless, one-shot, or task entry
name.sock   socket entry: stateful, multi-turn, streaming
name.d/     control directory: config, state, permissions, logs
```

If an object does not support stateful interaction, do not expose `name.sock`.
A socket that only reports errors is bad ABI.

Agent and tool control directories may contain a hook convention subtree:

```text
name.d/
  hooks/
    pre.d/
    post.d/
```

`pre.d` contains hooks that run before the object action; `post.d` contains
hooks that run after the object action. This is an object-local convention under
`agent` and `tool`; it does not create a `/ctx/hook` root namespace. Model
control directories stay limited to provider/model controls and do not carry
empty hook trees. v1 defines the directory shape only. Implementations may keep
compiled hook state in process memory, but development refresh is still a Git
commit/runtime restart boundary; CortexFS v1 does not define background
watchers, polling, or hot reload.

Do not expand one object into `profile/`, `runtime/`, `policy/`, `control/`,
and other layered trees. If a small file can say it, put it in `.d/`.

## Names and Aliases

ABI names under `agent` and `tool` are single path components. Model names are
the exception: `/ctx/model` uses the original provider/model namespace as two
path components.

Agent, tool, provider, and model path-component syntax:

```text
[a-zA-Z0-9][a-zA-Z0-9._+-]{0,63}
```

Forbidden:

```text
/
NUL
empty string
.
..
control characters
newline
suffix .sock
suffix .d
```

Rules:

```text
agent/tool filename is the stable alias
model stable identity is provider/model using the original model provider
aggregator name, API format, and base URL do not enter stable model names
native ids may live in .d/id, .d/driver, or another control file
short user aliases use symlinks
```

Example:

```text
/ctx/model/openai/gpt-4o
/ctx/model/openai/gpt-4o.d/id = openai/gpt-4o

/ctx/home/1000/model/main -> /ctx/model/openai/gpt-4o
```

Alias resolution:

```text
symlink means symlink
readlink decides object identity
there is no alias.d override semantics
one path is not half alias and half real object
```

If `coder` needs its own default parameters, create a real object instead of a
symlink overlay:

```text
/ctx/model/openai/gpt-4o-coder
/ctx/model/openai/gpt-4o-coder.d/id
/ctx/model/openai/gpt-4o-coder.d/default
```

## Exec Protocol

Executable agents may select their launch protocol with the optional
`agent/<name>.d/abi` control. The only v1 values are `argv-v1` and
`sdk-envelope-v1`. Absence is exactly equivalent to `argv-v1`, preserving the
legacy argv/stdin behavior without inference or content sniffing.

`model/<provider>/<model>`, `agent/<name>`, and `tool/<name>` are executable
files. They must accept argv or stdin input:

```bash
/ctx/model/openai/gpt-4o "hello"
echo "hello" | /ctx/model/openai/gpt-4o
echo '{"messages":[{"role":"user","content":"hello"}]}' | /ctx/model/openai/gpt-4o

/ctx/agent/coder "fix this project"
echo '{"task":"fix tests"}' | /ctx/agent/coder

/ctx/tool/fs.read '{"path":"README.md"}'
echo '{"path":"README.md"}' | /ctx/tool/fs.read
```

stdout should be JSONL:

```jsonl
{"type":"start","run":"r1","model":"openai/gpt-4o"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"done","run":"r1","status":"ok"}
```

Human-readable output can exist as compatibility mode. Machine callers should
prefer JSONL.

Reading an executable object returns inspectable metadata, not implementation
code. Built-in model and tool executable files use the common CortexFS object
runner shebang:

```text
#!/usr/bin/cortexfs-object-runner
```

`tool/<name>` must not expose a per-tool shell script as file contents. Tool
implementation dispatch is runtime behavior behind the common runner; `name.d/`
remains the inspectable control surface.

For installed executable plugins, the backing source keeps the verified
artifact bytes needed for `execve`, while the FUSE read projection uses the
object class and `.d` controls to return canonical inspectable metadata through
`object_exec_metadata`. Reading `/ctx/tool/<name>` or `/ctx/agent/<name>`
therefore does not reveal the plugin binary or source implementation.

Exit codes:

```text
0   success
1   generic error
2   bad arguments or bad input format
13  permission denied, maps to EACCES
69  service unavailable, object exists but runtime is unavailable
70  internal error
```

If stdout has already started emitting JSONL, errors should continue as JSONL
error frames. The exit code is only the process-level summary.

TTY rules:

```text
model/<provider>/<model>  with no args on a TTY may enter a simple REPL, but is not required to
agent/<name>              with no args on a TTY must enter an interactive socket session
tool/<name>               with no args on a TTY should print short usage or read stdin, not start a long session
```

## Socket Protocol

`model/<provider>/<model>.sock` and `agent/<name>.sock` are Unix domain
sockets. The protocol is JSONL.

Requests:

```jsonl
{"op":"send","id":"msg-1","session":"default","cwd":"/workspace","input":"hello"}
{"op":"resume","session":"default","after":"event-id"}
{"op":"cancel","id":"run-id"}
{"op":"ping"}
```

Responses:

```jsonl
{"type":"start","id":"event-id","run":"run-id","model":"openai/gpt-4o"}
{"type":"delta","id":"event-id","run":"run-id","text":"..."}
{"type":"message","id":"event-id","run":"run-id","role":"assistant","content":[{"type":"text","text":"..."}]}
{"type":"error","run":"run-id","code":"EACCES","message":"permission denied"}
{"type":"done","run":"run-id","status":"ok"}
{"type":"pong"}
```

Socket lifecycle:

```text
missing        object does not support stateful mode, or service is not started
ECONNREFUSED   object declares a socket, but the process is unavailable
connected      requests and responses are JSONL frames
closed         private/shared sessions are not deleted
```

The durable agent object may use `agent/<name>.sock` as a stopped placeholder.
When started, the visible path is an owner-authorized symlink to the live socket
below `/run/user/<uid>/cortexfs/agent/`. Terminal sessions similarly expose
`home/<uid>/agent/<name>/session/<session>/terminal/main.sock` as a symlink to
the matching runtime terminal socket. A start is not ready until both requested
visible aliases have been created and verified.

Hard socket rules:

```text
max frame size       v1 is 1 MiB; larger frames return EMSGSIZE
unknown fields       must be ignored
unknown op           returns EINVAL
after disconnect     private/shared sessions continue by default; temp sessions may be cancelled
client id retry      within one session, retry is idempotent and returns the original run id or final state
delta order          strictly increasing send order within one run
backpressure         blocking write is backpressure; implementation must not buffer forever in memory
```

When a client receives `SIGINT`, it should first send `cancel` to the socket.
Only the second interrupt, or a broken connection, should exit the client
process.

Error frames use stable errno names such as `EACCES`, `EINVAL`, `ENOENT`,
`EMSGSIZE`, and `EHOSTDOWN`. Clients must not parse natural language `message`.

## Installed Object Inspection and Removal

The host-side inspection and receipt-managed removal surfaces are:

```text
ctx object inspect --source PATH CLASS NAME [--tier user|system]
ctx object uninstall --source PATH CLASS NAME [--tier user|system] [--yes]
```

`CLASS` is `tool` or `agent`, and the tier defaults to `user`. Inspection
validates the installer receipt and its identity/version, the recorded
class/name/tier, the retained control directory's device/inode/type, and the
retained executable's device/inode/regular type, execute bits, and SHA-256. It
also rejects executable length, mode, mtime, or ctime changes observed during
inspection; the receipt does not bind the complete install-time mode. It holds
no-follow descriptors through validation and does not modify the backing tree.

Mutable control-file contents are outside this receipt claim and are not
revalidated against their install-time values. An object with a missing or
legacy receipt is unmanaged and is reported as unavailable; inspection does
not adopt it.

Uninstall accepts only one exact installer-receipt-managed `tool` or `agent`
pair, with tier defaulting to `user`. It is a no-write receipt validation and
report by default. `--yes` first quarantines the executable on the same
filesystem to establish the invisible-object boundary, syncs and rechecks its
receipt, then quarantines the control directory, syncs and rechecks both
receipts. Only after the complete exact stage is verified does it reuse bounded
residue cleanup. This is not pair atomicity.

At receipt checkpoints, a failure does not intentionally overwrite or delete a
foreign replacement and may retain audit-visible safety residue. Before
`--yes`, the caller must quiesce the matching agent runtime and every writer
sharing the same Unix authority. Receipt checkpoints do not close Linux's final
pathname syscall race against such a writer. Uninstall grants no authority,
creates no socket, and does not start or stop a runtime.

## Durable Safety Residue

Object installation may leave a hidden `.cortexfs-install-*` stage in the
install-class directory. Applied cleanup temporarily uses a
`.cortexfs-cleanup-*` quarantine, and agent creation rollback may leave
`.ctx-rollback-*`. These names are safety residue, not object names and not a
second submission or orchestration namespace.

The host-side maintenance surface is:

```text
ctx object residue audit --source PATH
ctx object residue cleanup --source PATH --path REL --dev DEV --ino INO [--yes]
```

Audit is a bounded, no-follow observation of a durable source. It reports
`.cortexfs-install-*`, `.cortexfs-cleanup-*`, and `.ctx-rollback-*` residue.
Its reported path, device, and inode are useful for review but do not grant
authority. Cleanup requires the caller to resubmit an explicit exact receipt
and then builds a complete bounded receipt plan before mutation. Audit fails
instead of silently skipping unreadable, cross-device, or over-limit subtrees.
Only `.cortexfs-install-*` directories directly below `tool`, `agent`,
`home/<decimal-uid>/tool`, or `home/<decimal-uid>/agent` are eligible. Cleanup
quarantine and rollback residue are always audit-only. Cleanup is dry-run
unless `--yes` is present.

Applied cleanup isolates each candidate with same-directory
`renameat2(RENAME_NOREPLACE)`, verifies the moved inode, and only then performs
descriptor-relative, post-order deletion. Symlinks are unlinked as leaves and
are never followed. If a later cleanup step fails, the original
`.cortexfs-install-*` name is restored only when the quarantined top-level inode
still matches the submitted receipt and no-replace restoration is safe. That
permits a fresh audit and retry. If safe restoration is impossible, the exact
retained `.cortexfs-cleanup-*` quarantine path is reported for later audit.
Unknown-type, depth/count, added-entry, and sync conflicts also stop cleanup.

Before `--yes`, the caller must quiesce processes that share write authority to
the backing directories. Linux does not provide receipt-conditioned unlink, so
no userspace design can close the final pathname syscall window against a
hostile writer in the same Unix authority boundary. At every receipt checkpoint
cleanup refuses to intentionally unlink a mismatched inode.

Rollback residue and retained cleanup quarantine are audit-only; only an
eligible install residue path can be submitted to cleanup. This command never
removes rollback residue. Owned agents are not residue. Install and agent-create
operations do not trigger automatic background residue cleanup.
