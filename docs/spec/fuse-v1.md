# FUSE v1 Shape

`/ctx` is the FUSE ABI view. The backend is intentionally not ABI.

The first implementation may use plain local state:

```text
~/.local/share/cortexfs/
  objects/
  sessions/
  logs/
  runtime/
```

FUSE projects that state into `/ctx`. Dynamic files may behave like `/proc`;
durable files may be backed by ordinary files. Clients must not care whether a
path comes from a local file, generated runtime state, or a later backend.

v1 FUSE should stay small:

```text
readdir
getattr
read
write small control files
atomic replace
read-only executable object projection
Unix socket path projection
session files
read-only CortexFS extended attributes
```

Do not add these to v1 FUSE:

```text
distributed backend
database backend
vector store
cluster runtime
provider registry
higher-level workflow runtime
hot reload command
background watcher as ABI
```

Development-triggered behavior is outside the filesystem ABI. Git commit is the
only project development trigger boundary; do not add root-level job, hook, or
workflow entrances.

## Dynamic and Durable

Path semantics stay simple:

```text
status                 dynamic
model/<provider>/<model>           dynamic executable entry
model/<provider>/<model>.sock      dynamic socket; existence means session=socket
model/<provider>/<model>.d/status  dynamic
model/<provider>/<model>.d/log     dynamic or durable, implementation choice
model/<provider>/<model>.d/id      durable or config projection
agent/<name>           dynamic executable entry
agent/<name>.sock      dynamic socket
agent/<name>.d/status  dynamic
agent/<name>.d/pid     dynamic
agent/<name>.d/owner   durable
agent/<name>.d/uid     durable
agent/<name>.d/gid     durable
agent/<name>.d/groups  durable
agent/<name>.d/label   durable
agent/<name>.d/iso     durable
agent/<name>.d/parent  durable
agent/<name>.d/root    durable
agent/<name>.d/cwd     durable
agent/<name>.d/env     durable
agent/<name>.d/path    durable
agent/<name>.d/mount   durable
agent/<name>.d/model   durable
agent/<name>.d/policy  durable
agent/<name>.d/log     dynamic or durable, implementation choice
tool/<name>            dynamic executable entry
tool/<name>.d/schema   durable
home/<uid>/model/*     durable alias or user model entry
home/<uid>/tool/*      durable user tool
home/<uid>/agent/*     durable user agent state
home/<uid>/            durable
shared/<name>/         durable
```

Clients do not get to depend on the backend choice.

The agent runtime-visible tool view may be a dynamic in-memory projection over
system, user, and shared tool source tiers. That projection is not durable
state and must not be represented by writing placeholder files or default
symlinks into `home/<uid>/tool`.

## Extended Attributes

FUSE exposes read-only `user.cortexfs.*` extended attributes so agents can
inspect a path before reading full contents:

```text
user.cortexfs.abi_path               ABI path relative to /ctx
user.cortexfs.kind                   stable ctx.* path classification
user.cortexfs.origin                 virtual, disk, or overlay
user.cortexfs.storage                memory or disk
user.cortexfs.virtual                true or false
user.cortexfs.backing_exists         true or false
user.cortexfs.backing_path           implementation path, when one exists
user.cortexfs.bytes                  projected byte size
user.cortexfs.token_estimate         fast token estimate for read planning
user.cortexfs.input_token_estimate   estimated input cost if read into context
user.cortexfs.output_token_estimate  estimated generated output; 0 when unknown
user.cortexfs.cache_bytes            cached bytes known to CortexFS; 0 when none
user.cortexfs.cache_entries          cache entry count; 0 when none
user.cortexfs.cache_state            none, partial, warm, or stale
user.cortexfs.tokenizer              tokenizer/estimator id
```

`origin=virtual storage=memory` means the file is projected by CortexFS rather
than read from a durable backing file. `origin=disk storage=disk` means the
visible content comes from the backing filesystem. `origin=overlay
storage=memory` is used for runtime overlays such as live socket paths.

Token counts are estimates unless a runtime later writes exact tokenizer
metadata. The default estimator is `byte-estimate-v1`, a cheap read-before-read
heuristic that does not scan full file contents. These xattrs are not control
files; `setxattr` and `removexattr` must fail.
