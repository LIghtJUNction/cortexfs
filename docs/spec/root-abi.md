# Root ABI

Root ABI is frozen for v1.

Stable root:

```text
/ctx/
  status
  bin/
  model/
  agent/
  tool/
  home/
  shared/
    cortexfs-docs/
      README.md
      man/
        ctx.agent.md
        ctx.tool.md
        ctx.model.md
        ctx.coreutils.md
        ctx.root-abi.md
        ctx.session.md
        ctx.provider.md
```

`/ctx/shared/cortexfs-docs/man/*.md` are documentation mirrors and should match the
corresponding normative `docs/spec/*.md` texts after documentation or ABI
changes. Refresh them from the matching `ctx` release source tree with
`ctx bootstrap` so runtime helpers do not keep stale references. If references
stay stale, the installed `ctx` binary is still serving an older embedded manual
bundle; install the matching binary build and rerun bootstrap.

Meaning:

```text
status   current mount status
bin/     CortexFS ABI helper commands
model/   system executable model entries, visible to all users by default
agent/   system executable agent entries, visible to all users by default
tool/    system executable tool entries, visible to all users by default
home/    CortexFS home for Linux users
shared/  shared space for users and agents
```

Resource tiers:

```text
/ctx/model              system models
/ctx/agent              system agents
/ctx/tool               system tools
/ctx/home/<uid>/model   user-specific models and aliases
/ctx/home/<uid>/agent   user-specific agent state and user agents
/ctx/home/<uid>/tool    user-specific tools
```

The system tiers are durable shared resources. The user tiers are durable
per-user resources. An agent's runtime-visible tool set is a separate in-memory
FUSE projection derived from its controls, policy, mounts, and Linux identity;
it is not materialized by copying or symlinking tools into the durable user
tree.

## v1 Reference Tree

This is the normative v1 shape. Concrete object names such as `debug/echo`,
`openai/gpt-5.6`, `base`, `coder`, `reviewer`, `executor`, `worker`, `1000`, and
`project-a` are examples of valid entries.

```text
/ctx/
  status

  bin/
    ctx
    ctxterm
    tsh

  model/
    main -> /ctx/model/openai/gpt-5.6
    helper -> /ctx/model/openai/codex-auto-review
    fast -> /ctx/model/openai/gpt-5.6
    reason -> /ctx/model/openai/gpt-5.6
    code -> /ctx/model/openai/gpt-5.6
    vision -> /ctx/model/openai/gpt-5.6

    debug/
      echo
      echo.d/
        id
        driver
        cap
        effort
        default
        fallback
        session
        status
        log

    openai/
      gpt-5.6
      gpt-5.6.d/
        id
        driver
        cap
        effort
        default
        fallback
        session
        status
        log

  agent/
    architect
    base.sock
    architect.d/
      owner
      uid
      gid
      groups
      label
      iso
      parent
      life
      root
      cwd
      env
      path
      mount
      model
      abi
      policy
      status
      pid
      log
      meta.json

    coder
    coder.sock
    coder.d/
      owner
      uid
      gid
      groups
      label
      iso
      parent
      life
      root
      cwd
      env
      path
      mount
      model
      policy
      status
      pid
      log
      meta.json

    reviewer
    reviewer.sock
    reviewer.d/
      owner
      uid
      gid
      groups
      label
      iso
      parent
      life
      root
      cwd
      env
      path
      mount
      model
      policy
      status
      pid
      log
      meta.json

    executor
    executor.sock
    executor.d/
      owner
      uid
      gid
      groups
      label
      iso
      parent
      life
      root
      cwd
      env
      path
      mount
      model
      policy
      status
      pid
      log
      meta.json

    worker
    worker.sock
    worker.d/
      owner
      uid
      gid
      groups
      label
      iso
      parent
      life
      root
      cwd
      env
      path
      mount
      model
      policy
      status
      pid
      log
      meta.json

  tool/
    fs.read
    fs.read.d/
    tsh
    tsh.d/
      name
      description
      schema
      cap
      policy
      status
      log
      config
    tsh.config
    tsh.config.d/
      name
      description
      schema
      cap
      policy
      status
      log
    bash
    bash.d/
    tmux
    tmux.d/
    zellij
    zellij.d/
      name
      description
      schema
      cap
      policy
      status
      log

    fs.write
    fs.write.d/
      name
      description
      schema
      cap
      policy
      status
      log

    shell.exec
    shell.exec.d/
      name
      description
      schema
      cap
      policy
      status
      log

  home/
    1000/
      agent/
        base/
          root/
          session/
            index/
              by-cwd/
              by-hash/
              by-uuid/

        coder/
          root/
          session/
            index/
              by-cwd/
              by-hash/
              by-uuid/

          data/
          cache/
          log/

      tool/

      model/
        main -> /ctx/model/openai/gpt-5.6

  shared/
    project-a/
      data/
      tool/
        project.test
        project.test.d/
          schema
          policy
          status
          log

      agent/
        coder/
          session/
            index/
              by-cwd/
              by-hash/
              by-uuid/

      queue/
        inbox/
        pending/
        lease/
        claimed/
        done/
        failed/

      result/
```

`tool/<name>.d/origin` is an optional diagnostic file, not stable ABI. Strict
clients must not depend on it.

No other v1 root entries are stable ABI. These are explicitly not root
directories:

```text
provider/
format/
db/
vector/
memory/
mcp/
skill/
cluster/
chan/
job/
hook/
workflow/
audit/
control/
space/
spawn/
factory/
agent-template/
AGENTS.rc
```

Some of those concepts may appear as internal implementation, higher-level
agent capability, tool capability, legacy convenience, or object-local `.d/`
diagnostics. They must not become root namespaces or CortexFS-defined framework
configuration formats.

MCP is specifically a tool source, not a root object. MCP-backed capabilities
may be exposed as ordinary tools such as `tool/github.search_issues`, where the
name is exactly `<server>.<remote_tool>`. Projection only writes manifest
candidates; installation requires explicit `ctx object check` and
`ctx object install --source ...`, and it grants no authority. CortexFS does
not define MCP server configuration files or formats. Those are ordinary files
visible through the agent view; execution still goes through `tool/`,
`CTX_PATH`, and policy.

The root rule:

```text
root only contains stable object classes
root never mirrors provider, database, workflow, memory, or orchestration internals
```

## Linux Shape

CortexFS follows ordinary Linux habits:

```text
small root       stable object classes only
executable obj   if it can run, it is first an executable file
side control     control files live in the matching .d/
small text       one value per file, one item per line for lists
streams          multi-turn, realtime, or long output uses .sock or stdout JSONL
permissions      uid/gid/mode/namespace first, label/policy second
explicit failure file ops return errno, exec returns exit code, sockets return error frames
```

Do not turn CortexFS into a directory mirror of an AI platform database. It
should look closer to a mix of `/bin`, `/dev`, `/proc`, and `/sys`: paths are
ABI, not product navigation.

## File Formats

Control files use the smallest format that works:

```text
single value      UTF-8 text ending in newline
list              one UTF-8 item per line
boolean           0 or 1
key/value         KEY=VALUE, one pair per line
event stream      JSONL
complex object    JSON, only when nesting is actually needed
```

Update control-plane files by same-directory atomic replacement: write a
temporary file, then `rename` it into place. Interactive chat uses sockets. Do
not fake chat with file rename.

## Environment

```sh
export CTX_ROOT=/ctx
export CTX_HOME="$CTX_ROOT/home/$(id -u)"
export CTX_PATH="$CTX_ROOT/tool:$CTX_HOME/tool"
export PATH="$CTX_ROOT/bin:$PATH"
```

Semantics:

```text
CTX_ROOT  CortexFS mount root
CTX_HOME  CortexFS home for the current Linux user
CTX_PATH  tool lookup path for agents and tools, similar to PATH
PATH      normal shell command path, may include /ctx/bin
```

`CTX_PATH` is only for tool lookup. It is not used to find models or agents.

When a human starts standalone `tsh`, it reads the user startup file before
inherited process `CTX_PATH` when the file exists:

```text
/ctx/home/<uid>/.tshrc
```

The file is data, not shell. The stable line format is:

```text
CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool
```

Blank lines and `#` comments are ignored. `tsh` must not execute `.tshrc` and
must not process `export`. When `.tshrc` provides `CTX_PATH`, it overrides
inherited process `CTX_PATH`; otherwise `tsh` falls back to inherited
`CTX_PATH`, then to the default path.

Inside an agent terminal, the runtime-provided process `CTX_PATH` remains
authoritative and is not overridden by `.tshrc`.

Stable ABI expresses agent environment as data files:

```text
/ctx/agent/<name>.d/env    KEY=VALUE, one per line
/ctx/agent/<name>.d/path   tool path, one per line
/ctx/agent/<name>.d/mount  bind mount table
```

Rules:

```text
config files are data
env does not do shell expansion
path builds the agent runtime CTX_PATH
mount builds the agent mount namespace
```
