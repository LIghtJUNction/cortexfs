# Phase 1 Migration Target

Phase 1 builds the smallest useful `/ctx` FUSE ABI view and the `ctx`
coreutils client.

It does not build:

```text
distributed backend
database backend
vector store
provider registry
workflow engine
cluster runtime
CortexFS AI API compatibility layer
```

Model calls enter through the Rig abstraction.

Start from this loop:

```text
/ctx/status
/usr/bin/ctx or ~/.local/bin/ctx
/ctx/bin/ctx
/ctx/model/debug/echo
/ctx/model/debug/echo.d/id
/ctx/model/debug/echo.d/driver
/ctx/model/debug/echo.d/cap
/ctx/model/debug/echo.d/default
/ctx/model/debug/echo.d/session
/ctx/model/debug/echo.d/status
/ctx/model/debug/echo.d/log
/ctx/agent/coder
/ctx/agent/coder.sock
/ctx/agent/coder.d/{owner,uid,gid,groups,label,iso,parent,life,root,cwd,env,path,mount,model,system.md,prompt.template.md,policy,status,pid,log,meta.json}
/ctx/tool/fs.read
/ctx/tool/fs.write
/ctx/tool/shell.exec
/ctx/home/<uid>/agent/coder/session/
/ctx/home/<uid>/agent/coder/session/index/
```

Phase 1 validates only:

```text
/ctx FUSE mount works
ctx ls/which/file works
model exec one-shot works
model socket optional works
agent socket conversation works
session history readable as files
policy allow/refuse works
tool execution goes through agent only
cwd bind mount works
JSONL streaming works
errno-style errors work
```

Not migrated yet:

```text
agent shell
complex MCP lifecycle
complex skill registry
database/vector database backend
cluster scheduling
training data export
full policy language
```

Those can return later as agent capability, tool capability, or internal
implementation. They do not get new root directories.
