# 根 ABI

`/ctx` 根 ABI 在滚动更新参考树时保持稳定。

稳定根目录：

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

`/ctx/shared/cortexfs-docs/man/*.md` 是文档镜像。`docs/spec/*.md` 的规范变更后应保持一致。更新镜像时从匹配的 `ctx` 发布树执行 `ctx bootstrap`，避免运行时继续引用旧嵌入手册；若仍是旧内容，说明安装了旧版 `ctx`，需升级二进制并重跑 `ctx bootstrap`。

含义：

```text
status   当前挂载状态
bin/     CortexFS ABI 辅助命令
model/   系统可执行模型条目，默认对所有用户可见
agent/   系统可执行 agent 条目，默认对所有用户可见
tool/    系统可执行工具条目，默认对所有用户可见
home/    Linux 用户的 CortexFS home
shared/  用户与 agent 共用空间
```

资源分层：

```text
/ctx/model              系统模型
/ctx/agent              系统 agent
/ctx/tool               系统工具
/ctx/home/<uid>/model   用户模型与别名
/ctx/home/<uid>/agent   用户 agent 状态与用户创建的 agent
/ctx/home/<uid>/tool    用户级工具
```

系统层是持久共享资源，用户层是按用户持久资源。agent 的运行时可见工具集合是独立的内存 FUSE 投影，由其控制文件、策略、挂载和 Linux 身份推导；不会通过复制或符号链接写入用户持久树。

## 稳定参考树

这是规范定义的稳定形状。如下对象名如 `debug/echo`、`openai/gpt-5.6`、`base`、`executor`、`reviewer`、`executor`、`worker`、`1000`、`project-a` 都是有效示例。

```text
/ctx/
  status

  bin/
    ctx
    ctxterm
    tsh

  model/
    main -> /ctx/model/openai/gpt-5.6
    helper -> /ctx/model/openai/gpt-5.6-sol
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
        limit
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
        limit
        session
        status
        log

  agent/
    architect
    architect.sock
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
      abi
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
      abi
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
      abi
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
      abi
      policy
      status
      pid
      log
      meta.json

  tool/
    fs.read
    fs.read.d/
      name
      description
      schema
      cap
      policy
      status
      log
    fs.list
    fs.list.d/
      name
      description
      schema
      cap
      policy
      status
      log
    fs.stat
    fs.stat.d/
      name
      description
      schema
      cap
      policy
      status
      log
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
      name
      description
      schema
      cap
      policy
      status
      log
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

        executor/
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
        executor/
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

`tool/<name>.d/origin` 是可选诊断文件，不是稳定 ABI。严格客户端不得依赖。

除此之外的根条目均不是稳定 ABI：

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

上述概念可能仅作为内部实现、顶层能力、工具能力、兼容性便利文件或对象本地 `.d/` 诊断出现，不可扩展为根命名空间或 CortexFS 框架配置格式。

MCP 只表示工具来源，不是根对象。基于 MCP 的能力可投影为普通工具，例如 `tool/github.search_issues`，名称固定为 `<server>.<remote_tool>`。投影仅写入 manifest 候选；安装必须由 `ctx object check` 与 `ctx object install --source ...` 显式执行，且不授予任何权限。CortexFS 不定义 MCP 服务器配置文件及格式，配置发现由普通文件通过代理视图提供，执行仍必须经过 `tool/`、`CTX_PATH` 与 policy。

根约束：

```text
root 只包含稳定对象类
root 不应镜像 provider、database、workflow、memory 或编排内部实现
```

## Linux 形态

CortexFS 遵循普通 Linux 风格：

```text
small root       仅包含稳定对象类
executable obj   可执行对象为可执行文件
side control     控制文件位于同名 .d/
small text       单值文件一行，列表文件每行一项
streams          多轮、实时或长输出使用 .sock 或 stdout JSONL
permissions      先 UID/GID/权限/命名空间，再 label/policy
errors           文件操作失败返回 errno；exec 返回退出码；socket 返回错误帧
```

不要将 CortexFS 变成 AI 平台数据库镜像。应接近 `/bin`、`/dev`、`/proc`、`/sys` 的混合：路径本身是 ABI，不是产品导航。

## 文件格式

控制文件采用最小可行格式：

```text
single value      UTF-8 文本，LF 结尾
list              UTF-8 列表，每行一项
boolean           0 或 1
key/value         KEY=VALUE，每行一个键值对
event stream      JSONL
complex object    仅在确实需要嵌套时使用 JSON
```

控制平面文件更新采用同目录原子替换：写入临时文件，再 `rename` 到位。交互聊天走套接字，不可用文件重命名“模拟聊天”。

## 环境

```sh
export CTX_ROOT=/ctx
export CTX_HOME="$CTX_ROOT/home/$(id -u)"
export CTX_PATH="$CTX_ROOT/tool:$CTX_HOME/tool"
export PATH="$CTX_ROOT/bin:$PATH"
```

语义：

```text
CTX_ROOT  CortexFS 挂载根
CTX_HOME  当前 Linux 用户的 CortexFS home
CTX_PATH  agent 与 tool 的查找路径，类似 PATH
PATH      普通 shell 命令路径，可包含 /ctx/bin
```

`CTX_PATH` 仅用于工具查找，不用于模型或 agent 查找。

当人类启动独立 `tsh` 时，如存在用户启动文件 `/ctx/home/<uid>/.tshrc`，先读取该文件再读取继承的进程 `CTX_PATH`。

该文件仅数据，不是 shell。稳定行格式：

```text
CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool
```

空行和 `#` 注释会被忽略。`tsh` 不执行 `.tshrc`，也不处理 `export`。当 `.tshrc` 提供了 `CTX_PATH` 时覆盖继承值；否则 `tsh` 回退到继承 `CTX_PATH`，再回退到默认路径。

在 agent 终端内，运行时注入的进程 `CTX_PATH` 始终为权威，不会被 `.tshrc` 覆盖。

稳定 ABI 将 agent 环境表示为数据文件：

```text
/ctx/agent/<name>.d/env    KEY=VALUE，每行一个
/ctx/agent/<name>.d/path   工具路径，每行一个
/ctx/agent/<name>.d/mount  bind mount 表
```

规则：

```text
配置文件是纯数据
env 不执行 shell 展开
path 构建 agent 运行时 CTX_PATH
mount 构建 agent 挂载命名空间
```
