# 对象 ABI

每个可执行对象都遵循相同的三元组：

```text
name        可执行入口：无状态、一次性或任务入口
name.sock   套接字入口：有状态、多轮、流式
name.d/     控制目录：配置、状态、权限、日志
```

如果一个对象不支持有状态交互，请不要公开`name.sock`。
一个只报告错误的套接字是糟糕的 ABI。

代理和工具控制目录可能包含一个钩子约定子树：

```text
name.d/
  hooks/
    pre.d/
    post.d/
```

`pre.d` 包含在对象操作之前运行的钩子；`post.d` 包含
在对象操作之后运行的钩子。这是对象本地的约定
`agent` 和 `tool`；它不会创建 `/ctx/hook` 根命名空间。模型
控制目录保持限制在提供者/模型控制范围内，且不包含
空的钩子树。稳定的 ABI 仅定义了目录结构。实现可以保留
在进程内存中编译了钩子状态，但开发刷新仍然是 Git
提交/运行时重启边界；CortexFS 未定义后台
观察者、轮询或热重载。

不要将一个对象扩展到 `profile/`、`runtime/`、`policy/`、`control/` 等分层树。
若一个小文件即可表达该内容，请放在 `.d/` 下，而不要新建专用子树。

## 名称和别名

在 `agent` 和 `tool` 下的 ABI 名称是单路径组件。模型名称是
例外：`/ctx/model` 将原始提供者/模型命名空间作为两层路径使用。
路径组件。

代理、工具、提供者和模型路径组件语法：

```text
[a-zA-Z0-9][a-zA-Z0-9._+-]{0,63}
```

禁止：

```text
/
NUL
empty string
.
..
控制字符
换行
后缀 .sock
后缀 .d
```

规则：

```text
agent/tool 的文件名是稳定别名
模型稳定身份是 `provider/model`，使用原始提供者名
provider/model；聚合器名、API 格式和 base URL 不会进入稳定模型名
原生 ID 可放在 `.d/id`、`.d/driver` 或其他控制文件
短期用户别名必须用符号链接（symlink）
```

示例：

```text
/ctx/model/openai/gpt-5.6
/ctx/model/openai/gpt-5.6.d/id = openai/gpt-5.6

/ctx/home/1000/model/main -> /ctx/model/openai/gpt-5.6
```

别名解析：

```text
symlink 表示文件系统符号链接
readlink 决定对象身份
不存在 `alias.d` 覆盖语义
路径不能半别名半真实对象
```

如果 `coder` 需要它自己的默认参数，请创建一个真实的对象，而不是一个
符号链接覆盖：

```text
/ctx/model/openai/gpt-5.6
/ctx/model/openai/gpt-5.6.d/id
/ctx/model/openai/gpt-5.6.d/default
```

## 执行协议

可执行对象在 `agent/<name>.d/abi` 中使用必需控制文件。
其可接受值为 `sdk-envelope-v1`，运行时提供文档化的类型化调用定义，
详见 [代理运行时规范](agent-runtime.md)。

`model/<provider>/<model>` 与 `tool/<name>` 都是可执行文件，可接受
`argv` 或 `stdin` 输入。agent 可执行文件通过主机侧 SDK envelope 启动，
而不是直接用用户 `argv` 调用。用法：

```bash
/ctx/model/openai/gpt-5.6 "hello"
echo "hello" | /ctx/model/openai/gpt-5.6
echo '{"messages":[{"role":"user","content":"hello"}]}' | /ctx/model/openai/gpt-5.6

/ctx/tool/fs.read '{"path":"README.md"}'
echo '{"path":"README.md"}' | /ctx/tool/fs.read
```

stdout 应该是 JSONL：

```jsonl
{"type":"start","run":"r1","model":"openai/gpt-5.6"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"done","run":"r1","status":"ok"}
```

可执行对象必须发出规范的 JSONL 序列。原始可读
stdout 是无效的工具输出。

读取可执行对象会返回可检查的元数据，而不是实现内容
代码。内置模型和工具可执行文件使用通用 CortexFS 对象
运行器 shebang:

```text
#!/usr/bin/cortexfs-object-runner
```

`tool/<name>` 不应将每个工具的 shell 脚本作为文件内容公开。工具
实现调度是常见运行器背后的运行时行为；`name.d/`
仍然是可检查的控制表面。

对于已安装的可执行插件，后端源保持已验证状态
`execve` 所需的工件字节，而 FUSE 读取投影使用的是
对象类和 `.d` 控制以返回规范可检查的元数据
`object_exec_metadata`。阅读 `/ctx/tool/<name>` 或 `/ctx/agent/<name>`
因此不会泄露插件的二进制文件或源代码实现。

退出代码：

```text
0   成功
1   一般错误
2   参数错误或输入格式错误
13  权限拒绝，映射到 EACCES
69  服务不可用，对象存在但运行时不可用
70  内部错误
```

如果 stdout 已经开始输出 JSONL，错误也应该继续以 JSONL 的形式输出
错误帧。退出代码只是进程级别的总结。

TTY 规则：

```text
model/<provider>/<model>  在 TTY 下无参数时可进入简单 REPL，但不要求
agent/<name>              在 TTY 下无参数时必须进入交互式 socket 会话
tool/<name>               在 TTY 下无参数时应打印简短用法或读取 stdin，不应开启长会话
```

## 套接字协议

`model/<provider>/<model>.sock` 和 `agent/<name>.sock` 是 Unix 域
套接字。协议是 JSONL。

请求：

```jsonl
{"op":"send","id":"msg-1","session":"default","cwd":"/workspace","input":"hello"}
{"op":"resume","session":"default","after":"event-id"}
{"op":"cancel","id":"run-id"}
{"op":"ping"}
```

回应：

```jsonl
{"type":"start","id":"event-id","run":"run-id","model":"openai/gpt-5.6"}
{"type":"delta","id":"event-id","run":"run-id","text":"..."}
{"type":"message","id":"event-id","run":"run-id","role":"assistant","content":[{"type":"text","text":"..."}]}
{"type":"error","run":"run-id","code":"EACCES","message":"permission denied"}
{"type":"done","run":"run-id","status":"ok"}
{"type":"pong"}
```

套接字生命周期：

```text
missing        对象不支持有状态模式，或服务未启动
ECONNREFUSED   对象声明了 socket，但进程不可用
connected      请求/响应为 JSONL 帧
closed         私有/共享会话不会被删除
```

持久代理对象可以使用 `agent/<name>.sock` 作为停止状态下的占位符。
启动时，可见的代理套接字可为归属者授权的符号链接，
指向 `/run/user/<uid>/cortexfs/agent/` 下的运行中插座；
部分部署中也可直接使用 `/ctx/agent/<name>.sock`。终端会话路径
`/home/<uid>/agent/<name>/session/<session>/terminal/main.sock` 作为符号链接指向
运行时终端套接字。启动在两个可见别名均创建并可通过 readlink 验证前不得标记就绪。

硬插座规则：

```text
max frame size       最大 1 MiB；更大帧返回 EMSGSIZE
unknown fields       必须忽略未知字段
unknown op           未知操作，返回 EINVAL
after disconnect     私有/共享会话默认继续；临时会话可取消
client id retry      同一会话内重试保持幂等，返回原始 run id 或最终状态
delta order          同一 run 内 send 顺序必须严格递增
backpressure         阻塞写入表示反压；实现不得在内存中无限缓存
```

当客户端接收到 `SIGINT` 时，应先向套接字发送 `cancel`。
只有第二次中断或连接中断才退出客户端进程。

错误帧使用稳定的 errno 名称，例如 `EACCES`、`EINVAL`、`ENOENT`、
`EMSGSIZE`，和 `EHOSTDOWN`。客户不得解析自然语言 `message`。

## 可执行对象清单

旧版清单模式仍然严格：

```text
`cortexfs.object/v1` 不接受 `version` 与 `compatibility`
```

`version` 和 `compatibility` 在 v1 上被拒绝而不是被忽略。一个 v2
清单必须同时提供这两个字段：

```json
{
  "schema": "cortexfs.object/v2",
  "version": "0.1.0",
  "compatibility": {
    "cortexfs": ">=0.1.7, <0.2.0"
  },
  "class": "tool",
  "name": "example.echo",
  "executable": {
    "path": "target/release/example-echo-tool",
    "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
  },
  "controls": {
    "description": "Echo text.",
    "schema": "{\"type\":\"object\"}",
    "cap": "text",
    "policy": ""
  }
}
```

`version` 是一个对象 SemVer。`compatibility.cortexfs` 是 Cargo 风格
SemVer 要求。`ctx object check` 和 `ctx object install` 都符合该要求
针对当前编译的 CortexFS 软件包版本的要求
`ctx`；不匹配是无效输入，退出 2，并且不执行写入操作。未知
这两个模式的字段仍然被拒绝。

一张 `cortexfs.object-install/v2` 收据会记录 `object_version`
和 `cortexfs_requirement`；`cortexfs.object-install/v1` 收据不记录这些字段。
检查报告仅用于兼容性事实和审计，不授予权限，也不启动运行时。
若后续 CortexFS 版本与记录要求不匹配，不得仅因该不匹配而阻止对受控对象的卸载。

安装仍然仅限于新对象。由收据管理的替换是单独的
具有强制 v2 候选的生命周期操作：

```text
ctx object replace --source PATH MANIFEST [--tier user|system] [--yes]
ctx object upgrade --source PATH MANIFEST [--tier user|system] [--yes]
ctx object rollback --source PATH MANIFEST [--tier user|system] [--yes]
```

`replace` 接受现有的 receipt-managed v1 或 v2 对象，并且不施加任何限制
版本排序，因此它是从旧版本v1收据到v2的迁移路径。
`upgrade` 需要一个现有的 v2 对象以及一个严格更高的候选 v2
版本。`rollback` 需要一个现有的 v2 对象，并且严格更低
候选 v2 版本。CortexFS 不保留版本历史记录：回滚调用者
必须提供旧的 v2 清单及其精确的哈希绑定工件。

所有三个命令默认为 dry-run。不写入；`--yes` 才执行。
候选清单必须精确指定待安装类/名称，必须通过当前 CortexFS 的兼容性校验，
且必须使用
`cortexfs.object/v2`。兼容性仍然不赋予任何权威。

替换阶段在同一文件系统内执行，先隐藏旧可执行文件，再切换到收据一致的候选项，
最后将新可执行文件作为可见提交边界发布。若预提交失败且旧候选收据仍匹配，
会自动回滚到旧版本。在收据检查点不应故意覆盖或删除外部 inode；若冲突或失败，
安全恢复可能保留可被审计观察到的安全残留。

该协议不保证成对原子性，并不能消除 Linux 最终
`pathname` 系统调用与同权写入者的竞争窗口。
`--yes` 前，调用者必须静默匹配运行时与其他写入者。替换本身不保留版本档案，
不停止/不启动运行时，不授予策略权限，也不会创建 socket 状态。

## 已安装对象的更换、检查和移除

主机端检查和接收管理移除的表面有：

```text
ctx object inspect --source PATH CLASS NAME [--tier user|system]
ctx object uninstall --source PATH CLASS NAME [--tier user|system] [--yes]
```

`CLASS` 是 `tool` 或 `agent`，等级默认为 `user`。检查
验证安装程序收据及其身份/版本，已记录的
类/名称/等级，保留控制目录的设备/索引节点/类型，以及
保留的可执行文件的设备/索引节点/常规类型、执行位和 SHA-256。它
也拒绝在此过程中观察到的可执行文件长度、模式、修改时间或创建时间的更改
检查；收据并不绑定完整的安装时模式。它保持
通过验证的不跟随描述符，并且不会修改支持树。

可变控制文件内容不在此收据声明范围内，不会按安装时值重验。
缺失或遗留收据的对象不会被托管，检查会将其标记为不可用。
对于 v2 收据，检查会打印记录的对象版本和 CortexFS 要求。
检查不会重新运行安装兼容性验证。

卸载仅接受一个精确的安装程序收据管理 `tool` 或 `agent`
配对，层级默认为 `user`。这是一个不写入的收据验证，并且
默认报告。`--yes` 首先在相同位置隔离可执行文件
文件系统用于建立隐形对象边界，同步并重新检查其
收据，然后隔离控制目录，同步并重新检查两者
收据。只有在完全准确的阶段被验证后，才会重新使用绑定的内容
残留物清理。这不是成对原子性。

在卸载收据检查点，如出现故障，不会故意覆盖或删除外部替换；
会保留可审计安全残留。`--yes` 执行前，调用者必须静默与匹配代理运行时及
同权写入者。收据检查点不能消除 Linux 最终 `pathname` 系统调用与写入者竞争。
卸载不授予任何权限，不创建套接字，不启动或停止运行时。它验证已安装收据和精确配对，
不会因为当前 CortexFS 版本与记录的 v2 要求不再匹配而拒绝该对象。

## 耐用安全残留

对象安装可能会在 `.cortexfs-install-*` 阶段留下隐藏
安装类目录。应用的清理暂时使用一个
`.cortexfs-cleanup-*` 隔离，以及代理创建回滚可能会留下
`.ctx-rollback-*`。这些名称是安全残留物，而不是对象名称，也不是一个
第二次提交或编排命名空间。

主机端维护界面是：

```text
ctx object residue audit --source PATH
ctx object residue cleanup --source PATH --path REL --dev DEV --ino INO [--yes]
```

审计是有限且无跟踪的持久化来源遍历。它会报告
`.cortexfs-install-*`、`.cortexfs-cleanup-*`和`.ctx-rollback-*`残基。
其报告的路径、设备和 inode 对审核有用，但并不授予权限
权限。清理需要调用者重新提交一个明确的精确收据
然后在变异之前构建一个完整的有限收据计划。审计失败
而不是默默跳过无法读取、跨设备或超限的子树。
只有直接位于 `tool`、`agent` 下面的 `.cortexfs-install-*` 目录，
`home/<decimal-uid>/tool` 或 `home/<decimal-uid>/agent` 有资格。清理
隔离和回滚残留总是仅供审核。清理是演练操作
除非存在`--yes`。

应用清理会先在同目录用 `renameat2(RENAME_NOREPLACE)` 将候选项隔离，
并验证移动后的 inode，再以后序方式删除。符号链接作为叶子解链，不会被跟随。
若后续清理步骤失败，且隔离态顶级 inode 与收据一致、可安全恢复，则恢复原位，
并允许重试；若无法安全恢复，会报告具体 `.cortexfs-cleanup-*` 检疫路径供复核。
未知文件类型、深度/计数限制、额外条目或同步冲突会阻止清理。

`--yes` 之前，调用方必须静默共享该备份目录写权限的并发进程。Linux
没有“仅在收据匹配时才 unlink”原语，因此同一 Unix 权限边界下敌对写入者可能竞争
最终路径名系统调用窗口。每个收据检查点清理都会拒绝故意 unlink 不匹配的 inode。

回滚残留与清理隔离目录仅供审核。只有符合条件的安装残留路径才可提交清理。
此命令不会移除回滚残留。仅清理可审计的残留，不会自动触发后台清理任务。
