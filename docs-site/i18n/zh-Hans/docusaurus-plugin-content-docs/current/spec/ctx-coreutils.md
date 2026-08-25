# ctx Coreutils

`ctx` 是面向 `/ctx` ABI 的轻量用户态客户端。

```text
ctx = CortexFS coreutils
```

它不是 AI 聊天产品，不是 daemon，不是 runtime，不是 provider SDK。`ctx` 的第一要务是证明 CortexFS ABI 可用普通 Unix 风格操作。

`ctx` 操作于 `/ctx`。它不必驻留在 `/ctx` 下；可安装在正常系统 `PATH`，例如 `/usr/bin/ctx` 或 `~/.local/bin/ctx`。

## 稳定命令

保持命令面保持精简：

```text
ctx status
ctx abi
ctx env
ctx root
ctx bootstrap
ctx mount

ctx ls
ctx ls model
ctx ls agent
ctx ls tool
ctx ls home
ctx ls shared/project-a

ctx which model openai/gpt-5.6
ctx which agent executor
ctx which tool fs.read

ctx path shared project-a
ctx agent history executor
ctx agent output executor
ctx agent resume executor --session default
ctx agent wait executor work-123 --session default

ctx agent new reviewer --model openai/gpt-5.6 --tool fs.read
ctx agent new reviewer --label reviewer_t --shared project-a:read --mount /work /work ro
ctx agent start reviewer
ctx agent stop reviewer
ctx agent status reviewer
ctx agent env reviewer
ctx agent ps
ctx agent chat reviewer
ctx agent send reviewer --approve example.echo "run the declared echo tool"

ctx terminal create executor [--session default] [--cwd /workspace]
ctx terminal list
ctx terminal status terminal-executor-default
ctx terminal watch terminal-executor-default
ctx terminal attach terminal-executor-default

ctx cat agent/executor.d/policy
ctx set agent/executor.d/cwd /work
ctx append agent/executor.d/path /ctx/tool
ctx file agent/executor.d/mount
ctx file type tool/fs.read
ctx file check agent/executor.d/mount
ctx schedule status home/1000/agent/executor/session/default/context/plan.json --done plan
ctx schedule advance home/1000/agent/executor/session/default/context/plan.json --done plan
ctx schedule claim home/1000/agent/executor/session/default/context/plan.json work-123
ctx schedule result home/1000/agent/executor/session/default/context/plan.json work-123 done "implemented"

ctx object check tool.yaml
ctx object install --source /var/lib/cortexfs/storage/current tool.yaml --tier system
ctx object inspect --source /var/lib/cortexfs/storage/current tool example.echo --tier system
ctx object upgrade --source /var/lib/cortexfs/storage/current tool-v2.yaml --tier system
ctx object rollback --source /var/lib/cortexfs/storage/current tool-v1.yaml --tier system
ctx object uninstall --source /var/lib/cortexfs/storage/current tool example.echo --tier system
ctx object residue audit --source /var/lib/cortexfs/storage/current
ctx object residue cleanup --source /var/lib/cortexfs/storage/current --path REL --dev DEV --ino INO

ctx validate-name executor
ctx doctor
```

不要添加：

```text
ctx provider
ctx mcp registry
ctx workflow
ctx memory
ctx vector
ctx cluster
```

像 `ctx send`、`ctx chat`、`ctx connect`、`ctx ping`、`ctx cancel` 这类 socket 便捷命令可存在，但必须作为同一套 socket ABI 的薄封装。

`ctx bootstrap [SOURCE]` 仅更新参考源树；它不会 remount `/ctx`、不会启动 watcher、不会新增二级刷新边界。

可选参数：

```text
ctx bootstrap --check [SOURCE]     报告 tree_version、缺失 agents、退休保留对象
ctx bootstrap --dry-run [SOURCE]   展示迁移与 reconcile/state 动作顺序（不写入）
```

默认 bootstrap 物化 `architect` / `executor` / `product-manager`，仅在状态变化时写入
`bin/cortexfs.bootstrap.json`（`schema`、`tree_version`、`managed_agents`、`applied_migrations`）。`base` / `coder` / `reviewer` / `worker` 已退休对象会被报告并保留人工复核，因为旧树无所有权清单与完整控制树完整性证明。`home/` 下的会话历史不被 bootstrap 删除。

顶层 agent session 快捷方式与 `ctx agent ...` 的当前会话默认行为一致：

```text
ctx history AGENT
ctx history AGENT --session SESSION
ctx resume AGENT
ctx resume AGENT --session SESSION
ctx send AGENT INPUT
ctx send AGENT --session SESSION INPUT
ctx agent wait AGENT CHILD [--session SESSION]
```

省略 `--session` 时先读 `session/index/current`，回退到 `default`。
`ctx send` 与 `ctx resume` 与 `ctx agent send`、`ctx agent resume` 渲染 assistant events 方式一致；raw socket JSONL 仅用于底层 socket 命令和显式 raw agent 模式。

`ctx agent chat` 是人类聊天 UI，通过 agent socket 工作，不是 agent terminal，也不会进入 `tsh`。人类观察/加入持久终端请用 `ctx agent watch` 或 `ctx agent attach`。

`ctx inspect agent/AGENT [--session SESSION]` 是统一只读调试视图。它展示 Agent 定义与 control 路径、当前实例摘要与收据存在性、所选持久会话、模型 hard limit 与 capabilities、policy 与 mount 路径、可见工具数量。字段均来源于现有控制文件、supervisor 收据和会话文件，不会创建实例、socket、会话或能力缓存。`ctx agent inspect AGENT [--session SESSION]` 是等价的 agent-domain 拼写。

`ctx agent send` 与 `ctx agent chat` 接受可重复 `--approve TOOL`。在非 raw 模式，客户端只对该显式列表内 exact tool name 响应 `allow_once`；其余全部拒绝。稳定协议不提供 blanket approval，也没有 TTY 提示。raw 客户端与无该 handler 的客户端将关闭写入一侧，因此在 `approval=ask` 时 fail closed。

`ctx agent wait` 是为 parent-owned 子结果通道提供非阻塞 waitpid 读取。它读取 `context/child/<child>/status`；`pending` 与 `active` 返回 service unavailable；`done`、`error`、`cancelled` 会打印 `child<TAB>status<TAB>agent<TAB>session<TAB>model<TAB>life<TAB>role`，随后打印 `result.md`。进程返回码遵循 child 状态：`done` -> 0，`error` -> 1，`cancelled` -> 130。不会轮询、不会启动 runtime、不会回收 history、不会删除 child 状态。

混合父代理调度使用显式单步命令：

```text
ctx schedule status PATH [--done NODE]...
ctx schedule advance PATH [--done NODE]...
ctx schedule claim PATH CHILD
ctx schedule result PATH CHILD done|error|cancelled RESULT [--refs-jsonl JSONL]
```

`PATH` 必须是代理会话下 `context/plan.json`。命令读取 parent agent 的 label 与 policy，依据 `context/child/<child>/status` 推导已完成委派节点，应用显式 `--done` 的节点 ID，物化新就绪的委派 handoff 到 `context/child/<child>/`。`ctx schedule status` 是同一状态的只读表。它打印：

`node<TAB>kind<TAB>agent<TAB>child<TAB>session<TAB>model<TAB>life<TAB>role<TAB>child_parent<TAB>state`

其中 `state` 为 `blocked`、`ready`、`pending`、`active`、`done`、`error`、`cancelled` 之一。
对于本地父节点，`session`、`model`、`life`、`role`、`child_parent` 列为 `-`。
已委派子节点显示显式 child session；若调度节点未带 session，则继承 parent session，并显示 selected backing agent model、生命周期与 backing parent ref。

对于委派子节点，后备 agent 必须同时存在 `agent/<name>` 与 `agent/<name>.d/`；调度命令不得为缺失 worker 默认补 `main`/`owned`。

每条输出的 `handoff` 包含：
- child `agent`
- `session`
- `agent/<name>.d/model` 选中的 model
- `agent/<name>.d/life`
- `role`
- shell-quoted 的 `parent='agent:<name> session:<session>'` 引用
- 及 `handoff`、`result`、`refs` 三个稳定 ABI 路径于 `context/child/<child>/` 下

父节点可将这些路径传给 worker，不需猜测输入路径、spark model 路径与生命周期；也无需推测写入位置。

`ctx schedule claim` 在 worker 声明接单后把 materialized child 通道标记为 `active`。它是从 `pending` 到 `active` 的单状态文件转换，幂等且不启动 runtime。输出行包含子 `agent`、`session`、selected `model`、`life`、parent reference，以及同样稳定的 `handoff`、`result`、`refs` 路径。

`ctx schedule result` 把子通道终态结果写回父会话：

`status`、`result.md`、`refs.jsonl`

输出包含 child `agent`、`session`、backing `model`、backing `life`、parent 引用，以及 `result` 与 `refs` 路径。
两个命令都不会启动 agent，不会后台循环，不会轮询，不会新增提交命名空间。

对象可扩展生命周期命令是薄包装：

```text
ctx object check MANIFEST
ctx object install --source PATH MANIFEST [--tier user|system]
ctx object inspect --source PATH CLASS NAME [--tier user|system]
ctx object replace --source PATH MANIFEST [--tier user|system] [--yes]
ctx object upgrade --source PATH MANIFEST [--tier user|system] [--yes]
ctx object rollback --source PATH MANIFEST [--tier user|system] [--yes]
ctx object uninstall --source PATH CLASS NAME [--tier user|system] [--yes]
```

`ctx object check` 只读，不需要 source tree；它执行与 install 之前相同的 manifest/control/type/executable mode/SHA-256 验证。通过时输出 `valid CLASS/NAME`。一次只接受一个 manifest 路径，不接受 install flags。

`--source` 是必需参数，指向可写的持久 backing tree。`/ctx`、`CTX_ROOT`、`--root` 是 ABI 映射，不可作为安装目标推断。`MANIFEST` 指定对象 class（`tool` 或 `agent`）、一个可执行文件路径与其 SHA-256，并提供 class controls。

旧 schema `cortexfs.object/v1` 不接受 `version` 或 `compatibility`；schema `cortexfs.object/v2` 要求 `version` 为对象 SemVer，`compatibility.cortexfs` 为 Cargo 风格 SemVer 约束。未知字段与控制项、符号链接、非普通文件、非可执行文件、摘要不符、已存在对象名均会被拒绝。
manifest 不可指定 command/args/wrapper/install tier。

`check` 和 `install` 都会比较 v2 要求版本与当前 `ctx` 的编译版本。若不匹配，输入无效并退出 2，不写入。版本兼容性是校验项，不是权限授予，也不启动 runtime。

Agent manifest 必须包含 `abi` 控制，且仅可为 `sdk-envelope-v1`。缺失或其他值在发布前即拒绝。

用户级工具安装在 `home/<decimal-uid>/tool`；系统级 tool/agent 安装在 `tool`/`agent`。根 ABI 保留 `home/<decimal-uid>/agent`，但清单 schema 不把 tier 带到 root socket runtime，故安装器会拒绝 user-tier agents，并引导到 system tier。安装不授予 policy。

安装会初始化 canonical runtime-owned `status/pid/log` 文件，但不创建 socket 状态。完整 control 目录先 staging 并 sync，随后以 no-replace 发布，最后在可见路径发布 executable 作为 commit 边界。发布前后会重复核对收据。成功或失败可能保留 `.cortexfs-install-*` 安全残留供后续清理。

`cortexfs.object-install/v2` receipt 记录 `object_version` 和 `cortexfs_requirement`；`cortexfs.object-install/v1` receipt 不记录。
安装仍然是 new-object-only；replace/upgrade/rollback 是明确的、由 receipt 管理的独立操作。

`ctx object inspect` 是只读检查某个精确受 installer 管理的 `tool` 或 `agent`，tier 默认 `user`。它校验安装 receipt 与身份版本、class/name/tier、保留 control 的设备/ino/类型、保留 executable 的设备/ino/普通文件类型/执行位和 SHA-256。执行期间检测到 executable 长度、mode、mtime、ctime 变化将拒绝；receipt 也不绑定完整安装时 mode。成功输出示例：

```text
installed CLASS/NAME tier=T schema=cortexfs.object/v1 sha256=HASH executable=DEV:INO control=DEV:INO
installed CLASS/NAME tier=T schema=cortexfs.object/v2 version=VERSION requires-cortexfs=REQ sha256=HASH executable=DEV:INO control=DEV:INO
```

检查不会声称控制文件当前内容仍与安装态相同。缺失或 legacy receipt 的对象视为 unmanaged 并报告为 unavailable；inspect 不应 adopt 或修改它。v2 下兼容值仅作事实记录；安装后若某些值变化，不会因兼容退化而直接拒绝对象。

`replace`、`upgrade`、`rollback` 均要求候选 manifest 为 v2，并默认 dry-run。`--yes` 执行转换。`replace` 接受 v1 或 v2 当前对象的 receipt；不要求版本序。`upgrade` 要求当前 v2 且候选版本严格更高；`rollback` 要求当前 v2 且候选版本严格更低，且 caller 提供旧清单与精确 artifact（因 CortexFS 不保留版本历史）。

成功输出可选一项：

```text
would-replace CLASS/NAME tier=T from=FROM to=TO
would-upgrade CLASS/NAME tier=T from=FROM to=TO
would-rollback CLASS/NAME tier=T from=FROM to=TO
replaced CLASS/NAME tier=T from=FROM to=TO
upgraded CLASS/NAME tier=T from=FROM to=TO
rolled-back CLASS/NAME tier=T from=FROM to=TO
```

`FROM` 对 v2 是 manifest 版本，对 v1 显示 `legacy`。替换/升级/回滚会在同文件系统构建候选 stage、先暂存旧 executable，再把新 executable 最后发布为可见 commit 边界。提交前任一失败自动安全恢复旧 pair。收据检查不应覆盖或删除 foreign inode；冲突时可能保留 audit-visible 安全残留。这不是严格的 pair atomicity，也不消除最终 pathname 竞态。

执行 `--yes` 前，调用方应先静止匹配 runtime 和其他 writer。该命令不启动/停止 runtime，不保留版本历史，不授予 policy，也不会创建 socket。

`ctx object uninstall` 只接受一个精确的 installer-managed `tool` 或 `agent` 对，tier 默认 `user`。dry-run 与 inspect 相同校验，不写入。成功会报告待/已删除的 executable 与 control 的设备/ino。

带 `--yes` 时，uninstall 先将 executable 在同文件系统改名到隔离路径形成不可见对象边界；sync 并复检 receipt；再隔离 control 并复检 receipts。仅在完整 exact stage 验证通过后才执行 bounded residue cleanup。这个顺序不宣称 pair atomicity；检查点失败不会故意覆盖或删除 foreign replacement。若安全恢复不可完成，可保留可审计的残留。

`--yes` 执行前也应先静止 matching runtime 与可写 backing 目录的同权 writer。收据检查不能关闭 Linux 的最终 pathname 原子竞态；这类竞态由调用方协同静默。uninstall 不会重复执行 v2 compatibility 验证，因此后续 `ctx` 版本 mismatch 不会卡死已管理对象。

持久 residue 维护与安装解耦：

```text
ctx object residue audit --source PATH
ctx object residue cleanup --source PATH --path REL --dev DEV --ino INO [--yes]
```

`audit` 进行 bounded、无 follow、基于文件描述符的遍历，按相对路径排序逐行报告 `.cortexfs-install-*`、`.cortexfs-cleanup-*`、`.ctx-rollback-*`，包括类型、路径、device、inode、文件类、空/占用、清理可行性。audit 只是观察，不授予 cleanup 权限；后续清理必须携带显式相对路径和精确 `dev`/`ino`。audit 不会静默跳过不可读、跨设备或超限子树；所以扫描系统 backing 需要具备读取完整目录树的权限。

Cleanup 仅允许处理 `tool/`、`agent/`、`home/<decimal-uid>/tool/`、`home/<decimal-uid>/agent/` 下的 install-stage 目录。默认 dry-run，输出 `would-clean ... entries=N`；`--yes` 才会提交并输出 `cleaned ... entries=N`。清理首先用同目录 no-replace 重命名隔离顶路径并校验 moved inode；再按预检后序处理每个子项，不跟随符号链接。隔离名为 `.cortexfs-cleanup-*`；保留于 cleanup 的隔离目录仅供审计，不可再次作为 cleanup 目标。任何后续 step 失败时会尝试在可恢复条件下回滚到原 `.cortexfs-install-*`。若恢复失败，错误会报告具体 `.cortexfs-cleanup-*` 路径。未知文件类型、遍历上限、新入口或 sync 失败也会终止 cleanup。

`--yes` 前应停止与背后目录共享写权限的并发进程。Linux 没有“若 dev/ino 未变化则 unlink”原语，因此 receipt 仍无法保护最终 syscall 窗口不被同权写入者争用。
Cleanup 不会主动删除 receipt mismatch。

`.ctx-rollback-*` 永远是 audit-only（可能保留回滚冲突的 inode）。保留的 `.cortexfs-cleanup-*` 同样 audit-only，只有符合条件的 `.cortexfs-install-*` 可被 cleanup 提交。此命令不会删 rollback residue 或已管理 agent 对象。安装不会自动触发 residue cleanup。

运行时，`CTX_SOURCE` 只是候选环境路径。持久写入必须由 runtime capability 收据鉴权，且 path 需匹配 nofollow 的 source 目录、device、inode 与普通目录类型。

## Top-level 会话与 agent 创建

```text
ctx agent new NAME [--temp] [--parent PARENT] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]
ctx agent new [NAME] --from PROFILE
ctx agent apply NAME --from PROFILE
ctx agent start NAME
ctx agent stop NAME
ctx agent status NAME
ctx agent env NAME
ctx agent ps
ctx agent children NAME
ctx agent wait NAME CHILD
```

`ctx agent new` 只会在完整的 agent runtime 上下文中（`CTX_AGENT`、`CTX_SESSION`、`CTX_RUN_ID`、`CTX_SOURCE`）调用 `/ctx/tool/agent.create`。
普通主机调用会直接通过写 `agent/<name>.d/*` 控制文件和 `home/<uid>/agent/<name>/` 骨架目录来创建标准 agent 对象，这属于 supervisor 操作，不授予 agent policy。
`ctx agent new --temp` 在两个入口均记录 `life=temp`。
`--parent` 记录标准 `agent/<name>.d/parent`，例如 `agent:executor session:default run:r1`；这样创建的 worker child 在 wait/stop 时会有可见 parent，而不会新增独立进程表命名空间。

`--from` 接受 host 侧 `agent.yaml`、包含一个文件的目录、或短名称 profile。`new/apply` 在写入普通 `.d/*` 控制前先校验 profile 字段。`apply` 保留未指定控制和未知 `meta.json` 键；拒绝符号链接控制与无效 profile/meta。

`ctx agent start` 会启动现有 agent 的显式运行时。runtime socket 就绪后，主机端写 `agent/<name>.d/status` 为 `ready` 并 append `agent.start` 到 `agent/<name>.d/log`。`start` 输出与 `agent.start` 事件会回显 `model`、`life` 与 `role`；`pid` 仍为数值文本，systemd invocation id 仅是日志事实。

`ctx agent stop` 在 tool 存在时调用 `/ctx/tool/agent.stop`；若缺失则主机 fallback 为 supervisor stop：写 `agent/<name>.d/status=dead`、清 `agent/<name>.d/pid`，append `agent.stop` 到 `agent/<name>.d/log`。同一 fallback 还会递归把 `owned` 或 `temp` 且 `parent` 指向被停止 agent 的子对象标记为 `cancelled`/`dead`，同时保留其历史与 control 可读。若子代理是某个待处理/进行中 `context/child/<child>/` 的后台引用通道 backing，fallback 需把父侧 child result 也记为 `cancelled`，供 `ctx agent wait` 观察终态。该过程不创建新生命周期命名空间或队列。

退役引用对象 `base`、`executor` 为人工审核对象：子发现不会读取其 legacy ownership 字段，不会修改它们的 controls/status/child result。任何 unit reset 或 control 写前，fallback stop 必须验证完整非退役子树、检测所有权环，并预检计划控制与现有 pending/active child-result 通道。执行时按 post-order（子先父后）进行。

`ctx agent status` 读取普通 `agent/<name>.d/*` 控制，先打印 status，再打印 `model=`、`life=`、`role=`、`parent=`、`children=`、`pid=`、`ppid=`、`uid=`、`gid=`、`groups=`、`root=`、`cwd=`。首行可直接作为 process state；同时暴露 backing model、worker role、直接子 agent 数、parent 关系与 Linux 身份、chroot/cwd 方便运维观察。`parent=` 与 `ctx agent ps` 一致为标准化 parent ref（含可选 `session` 和 `run`）。

`children` 计数只包括有效状态非 `dead` 的直接子对象；`ready` 或 `busy` 且 pid 过期的 child 与 `ps` 相同逻辑排除。

`ctx agent ps` 可直接读取 `agent/<name>.d/parent`、`model`、`life`、`status`、`pid` 产出进程树，含派生 worker role 与实时 `ppid`。

默认 `main` model 与 `owned` 生命周期可保持隐式；非默认 worker model 与非 `owned` 生命周期应体现在树中。

`ctx agent env` 与 start 所用相同运行时视图读取并输出 `KEY=value` 行。它是只读检查现有 control 文件，不用作 host 变量继承或状态变更入口。

`ctx agent children` 读取 parent session 下 `context/child/<child>/` 表并打印制表符分隔的字段：

`child`、child channel `status`、backing `agent`、child-channel `session`、`parent` session、`parent` run、backing agent `model`、backing agent `life`、`role`、backing agent `status`、live parent `ppid`、backing agent `pid`（无则为 `-`）。

`role` 从稳定 worker role 名称推导；其他 backing 列来自 `agent/<agent>.d/*` 控制。通过该视图可以观察 worker 的任务状态及其与父会话/run 附着关系，而不必复制运行时状态。

`ctx agent wait` 读取同一 child 通道。如果子仍 `active` 但 backing agent 当前有效状态 `dead`、无 live pid 且仍回指 parent agent/session，则 `wait` 会把 child 渠道记为 `cancelled` 并返回取消码。
`ready` 或 `busy` 且有 numeric pid 但 `/proc` 不存在时，也按无 live pid 处理。终态输出字段与 `ctx agent children` 相同，随后打印 `result.md`。这是同步 reap，不是后台 poller。

## 安装边界

`/ctx/bin` 仅保留 runtimes/ABI 级脚本与二进制：

```text
/ctx/bin/ctx
/ctx/bin/ctxterm
/ctx/bin/tsh
```

首个实现可能只暴露 `ctx`，但 agent terminal runtime 应优先使用 `ctxterm` 与 `tsh`。放置规则为：

```text
human CLI              system PATH，通常一个 ctx 二进制
agent capability       /ctx/tool
runtime ABI helper     /ctx/bin
```

`ctxterm` 是代理终端模拟器。它持有伪终端并默认启动 `tsh`。`tsh` 是在该终端中运行的工具 shell，通过 `CTX_PATH` 解析命令，不直接执行任意 host command。像 `bash` 这类命令只有在 `CTX_PATH` 可见并授权为 tool 时才可运行。

`ctx agent start <agent> --session <session>` 在 sandbox 中启动默认代理终端。默认将调用者当前目录以 rw 挂载到 `/workspace`；若该目录含 `.git`，将 `.git` 只读覆盖挂载到 `/workspace/.git`。代理进程启动目录为 `/workspace`，因此宿主路径不是 agent 的 `pwd`，代理看到的是 sandbox 映射。沙箱 home 为 `/home/agent`，后备于 `/ctx/home/<uid>/agent/<agent>`，因此 `.config`、`.cache`、`.bash_history` 不应写入项目工作区。

终端进程从空环境启动，allowlist 如 `CTX_ROOT`、`CTX_HOME`、`HOME=/home/agent`、`PATH=/usr/bin:/bin`、`USER`、`LOGNAME`、`SHELL`、`TERM`、`LANG`。主机会话变量和 secrets 不默认继承。

可显式追加 mounts：

```text
ctx agent start <agent> --session <session> \
  --mount /host/path /workspace rw \
  --mount /host/input /input ro \
  --cwd /workspace
```

`ctxterm --broker AGENT SESSION UNIT` 在启动 Agent 前注册 PTY supervisor。会话终端路径为：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

该 ABI path 统一指向 `/run/cortexfs/terminal/broker.sock`。`ctx agent watch` 与 `ctx agent attach` 先认证 root broker，再请求绑定模式、会话和一次性 nonce 的描述符授权；它们不会连接用户级终端监听器，也不会发送一行模式前缀。

对应人类命令：

```text
ctx agent watch <agent> --session <session>
ctx agent attach <agent> --session <session>
```

独立人类 `tsh` 会话会优先读取 `CTX_HOME/.tshrc`，再读进程 `CTX_PATH`（文件存在时）。该文件是纯数据文件，仅支持：

```text
CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool
```

在 agent terminal 内，`tsh` 使用运行时注入的进程 `CTX_PATH`（作为权威）。

不得将 `/ctx/bin` 变为第二套 `/usr/bin`。

## 路径模型

`ctx` 解析 `CTX_ROOT` 下路径，默认 `/ctx`：

```text
ctx ls agent
ctx cat model/openai/gpt-5.6.d/cap
ctx file type tool/fs.read
ctx exec agent/executor "fix tests"
```

对象字符串使用 ABI path：

```text
model/openai/gpt-5.6
agent/executor
tool/fs.read
```

## 核心命令

`ctx status` 读取 `/ctx/status`。

`ctx ls` 使用 `readdir`，接受 `CTX_ROOT` 下任意 ABI path，不传入路径时默认 root。不会查询数据库/索引/registry/daemon catalog。

`ctx which` 按 ABI class 查找可执行对象：

```text
ctx which model openai/gpt-5.6
ctx which agent executor
ctx which tool fs.read
```

`ctx tool NAME [ARG...]` 是允许列表内、与 CortexFS core tool CLI 兼容的窄入口，例如：

```text
ctx tool tsh.config
ctx tool tsh.config '{"max_loaded_tools":32}'
```

在调用 allowlist CLI 前，`ctx tool` 仍通过 `CTX_PATH` 解析 `NAME` 并要求对应 ABI 对象存在。它必须拒绝普通可见工具与 authority-bearing core tools（如 `fs.write`、`shell.exec`）；直接从 `CTX_PATH` 执行这些会绕过 CortexFS tool authorization。非 allowlist 的工具须通过 `tsh`、agent runtime 或其他授权执行路径运行。

`ctx cat` 读取 ABI 文件，不应承载过多解释语义。

`ctx set` 使用同目录原子替换；`ctx append` 仅用于可追加文件（如 newline list）。`ctx file check` 在 ABI 定义该解析语义时校验路径形状与文件语法。
`ctx set` 与 `ctx append` 不应改写 `messages.jsonl` 或 `events.jsonl`；会话历史由 runtime 或授权 FUSE writer 维护。

`ctx file` 通过 path shape、`stat`、`readlink` 与只读的 `user.cortexfs.*` xattr 检视 CortexFS 路径。它打印稳定的 type 字符串、投影字节大小、token 估算与可用 CortexFS xattrs，不查询 registry。

稳定类型字符串：

```text
ctx.model.exec
ctx.model.socket
ctx.model.control
ctx.agent.exec
ctx.agent.socket
ctx.agent.control
ctx.tool.exec
ctx.tool.socket
ctx.tool.control
ctx.session.dir
ctx.session.messages
ctx.session.events
ctx.shared.dir
ctx.shared.tool.exec
ctx.shared.tool.control
ctx.shared.queue
ctx.shared.result
ctx.home.dir
ctx.symlink
ctx.ordinary
ctx.unknown
```

## cd

外部进程不能改变其父 shell cwd。`ctx cd` 不应伪装成可以这么做。

正确的用法：

```bash
cd "$(ctx path shared project-a)"
```

若 `ctx cd` 存在，它仅作为 shell 集成辅助：

```bash
eval "$(ctx cd project-a --shell)"
```

## 会话

`ctx agent history`、`ctx agent output`、`ctx agent trajectory` 与 `ctx agent resume` 读取会话文件并连接相关 socket。它们不维护私有聊天数据库。

当未传 `--session` 时，先用 `session/index/current`，回退 `default`。`ctx latest` 不存在；当前会话行为属于 `--session` 省略。

示例：

```text
ctx agent history executor
ctx agent output executor
ctx agent trajectory executor
ctx agent resume executor --session default
```

命令读取：

```text
/ctx/home/<uid>/agent/<agent>/session/index/list
/ctx/home/<uid>/agent/<agent>/session/index/current
/ctx/home/<uid>/agent/<agent>/session/<session>/latest.md
/ctx/home/<uid>/agent/<agent>/session/<session>/messages.jsonl
/ctx/home/<uid>/agent/<agent>/session/<session>/events.jsonl
```

`ctx agent trajectory` 打印经过验证的 ATIF 投影，按 run/call 关联工具调用、观察与 token 使用；不会创建第二份持久历史。只会投影具有标准 `tool_call` 事件且 run/call 匹配的 tool result；无匹配的不投影。验证失败时 CLI 输出可操作问题点（step/result/call id），最多 16 条，其余计数另报。源/调用标识会转义用于终端输出，字段有上限并逐条截断到 256 字符。

## 终端

`ctx terminal` 面对的是持久终端资源而不是 agent 定义。list/status 检查会话本地元数据，watch 只读接入 PTY，attach 可写接入；PTY bytes 与进程退出事实会追加到事件流用于回放与调试。

当前 `create` 走 agent-backed 路径，重用现有 supervisor 启动链，不引入 `ctx/terminal` 根命名空间或 detached create bash supervisor；后者留待版本化 terminal ABI 修订。

## Provider OAuth

`ctx provider oauth` 是主机侧凭证助手，不新增 `/ctx/provider` 命名空间，也不通过 model 文件暴露 token。

```text
ctx provider oauth login PROVIDER [--timeout SECONDS]
ctx provider oauth status PROVIDER
ctx provider oauth refresh PROVIDER
```

`login` 读取 `/etc/cortexfs/providers.d/*.json`，使用 provider 的 `oauth` block，创建 PKCE `S256` 授权请求，等待配置 `localhost` 重定向地址，换取授权码后存入系统 keychain：

```text
service=cortexfs:<provider> account=oauth:access
service=cortexfs:<provider> account=oauth:refresh
```

## 非目标目标

`ctx` 不应：

```text
通过 /ctx 暴露 provider key
存储用户私密聊天历史
实现 tool calling
解析 OpenAI/Anthropic/Gemini 请求格式
维护 agent registry
修改 messages.jsonl 来模拟聊天
本地决定 policy
fallback 到其他模型
把运行时错误掩藏为产品文案
```

这些职责属于 CortexFS 协议适配层、agent runtime、tools 或 CortexFS ABI 本身。
