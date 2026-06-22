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
exec wrapper
Unix socket path projection
session files
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
model/<name>           dynamic executable entry
model/<name>.sock      dynamic socket; existence means session=socket
model/<name>.d/status  dynamic
model/<name>.d/log     dynamic or durable, implementation choice
model/<name>.d/id      durable or config projection
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
home/<uid>/tool/*      durable alias or user tool
home/<uid>/agent/*     durable agent data
home/<uid>/            durable
shared/<name>/         durable
```

Clients do not get to depend on the backend choice.
