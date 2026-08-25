# 会话 ABI

Socket 请求必须包含 `session`。如果客户端省略它，运行时将使用
`default`。

请求：

```jsonl
{"op":"send","id":"client-msg-id","session":"default","scope":"private","cwd":"/workspace","input":"hello"}
```

## 持久运行 ID

每个生产级 `send`、`chat` 和 `repl` 运行 ID 都独立生成，
由 128 位 Linux 系统熵编码为 `ctx-` 后接 32 个小写十六进制字符组成。
偶发重用或碰撞概率可忽略不计。示例和测试中的 `r1`、`run-1`、
`msg-1` 仅作说明或本地标签，不是生产环境可持久依赖的 ID。

在一次会话中，重试 `send` 且客户端 `id`、输入、`scope` 与有效
`cwd` 都一致时，会重放原始 `start` 或记录的最终 `done`。
重放不会执行代理，也不会附加任何消息、事件或索引事实。
使用不同有效负载复用 `id` 会返回 `EINVAL`。格式错误的 JSONL 或
最后一行缺少结尾换行符将返回 `EIO`；实现不得追加或复用无法证明的
历史记录。

`cwd` 必须是代理 chroot 内的路径。如果省略，运行时将使用
`agent/<name>.d/cwd`。如果`cwd`不存在，则返回`ENOENT`。如果它存在
但位于可见挂载/Chroot之外，返回 `EACCES`。客户端不得传递
一个主机绝对路径，以绕过代理根目录。

`scope` 有三个值：

```text
private  默认；对当前 Linux uid 私有，可恢复
shared   存放于 shared 空间，允许时对多个代理或用户可见
temp     临时会话，不要求在 socket 关闭或代理退出后仍保留
```

代理会话地点：

```text
private  /ctx/home/<uid>/agent/<agent>/session/<session>/
shared   /ctx/shared/<name>/agent/<agent>/session/<session>/
temp     无持久路径要求；可仅驻留进程内存
```

模型会话地点：

```text
private  /ctx/home/<uid>/model/<model>.d/session/<session>/
shared   /ctx/shared/<name>/model/<model>.d/session/<session>/
temp     无持久路径要求
```

## 会话目录

会话目录使用普通文件：

```text
messages.jsonl  会话消息
events.jsonl    工具调用、错误与状态变更
latest.md       最新 assistant 文本
state           active、idle、done、error
cwd             会话工作目录
created_at      创建时间
updated_at      更新时间
meta.json       客户端、模型、scope 与相关元数据
AGENTS.md       可选运行快照：生效的 AGENTS.md 合并规则
SKILLS.md       可选运行快照：仅已发现的 skill 元数据
context/        可重建提示词工作集与派生上下文缓存
```

`AGENTS.md` 和 `SKILLS.md` 是会话目录下的可观察性快照，
在代理运行时为某次运行构建提示时写入。它们不是必需的会话布局文件，
不得授予权限。

```text
AGENTS.md  注入 {{rules}} 的项目 AGENTS.md 与全局 AGENTS.md 合并文本
SKILLS.md  仅 skill 目录元数据（name、description、SKILL.md 路径）
```

完整技能体保持在`SKILLS.md`中列出的原始`SKILL.md`路径中。
快照是每次运行时以原子方式替换的普通文件；
旧的运行内容不会在会话目录中版本化。

`ctx agent trajectory <agent> [--session <session>]` 项目会将
`messages.jsonl` 和 `events.jsonl` 输出为经过验证的 ATIF JSON。
`run` 与工具调用 ID 仍是工具调用的关联权威。该投影是导出输出，
不是第二套持久历史或提交路径。

工具结果必须包含与规范 `tool_call` 匹配的运行和调用 ID
事件。投影会丢弃不匹配的结果，并且从不合成工具调用或
聊天消息。

历史是会话文件。不要添加`/ctx/history`。
上下文运行时状态保存在会话目录下。不要添加
`/ctx/memory`、`/ctx/context`、`/ctx/swap` 或 `/ctx/task`。

用户可以通过普通文件操作查看历史记录：

```bash
ctx agent history executor
ctx agent output executor
less /ctx/home/$(id -u)/agent/executor/session/default/messages.jsonl
cat /ctx/home/$(id -u)/agent/executor/session/default/AGENTS.md
cat /ctx/home/$(id -u)/agent/executor/session/default/SKILLS.md
```

如果省略 `--session`，客户端命令解析 `session/index/current`
首先回退到`default`。没有单独的`ctx latest`命令。

## 会话索引

保留索引文件位于 `session/index/` 下以避免
与用户碰撞
会话名称，例如`list`、`current`、`by-cwd`、`by-hash`或`by-uuid`。

```text
session/
  index/
    list
    current
    by-cwd/
      <hash>
    by-hash/
      <hash>
    by-uuid/
      <uuid>
  default/
    messages.jsonl
    events.jsonl
    latest.md
    state
    cwd
    created_at
    updated_at
    meta.json
```

索引文件格式是固定的：

```text
index/list            每行一个会话名，按 updated_at 最新优先
index/current         单值：当前会话名
index/by-cwd/<hash>   单值：该 cwd 对应会话名
index/by-hash/<hash>  单值：该外部 hash 对应会话名
index/by-uuid/<uuid>  单值：该外部 uuid 对应会话名
```

`index/by-cwd/<hash>`、`index/by-hash/<hash>` 和 `index/by-uuid/<uuid>` 均不是
符号链接，避免在不同挂载与不同后端之间出现 ABI 不一致。

会话垃圾回收默认处于“预览，不写入”模式。加 `--yes` 时会在同一文件系统内
使用 `RENAME_NOREPLACE` 将每个符合条件的活动会话存档到
`<CTX_HOME>/archived_sessions/<agent>/<session>`，并从
`index/list`、`index/by-cwd/`、`index/by-hash/`、`index/by-uuid/`
准确移除该会话的索引引用。归档目标不能覆盖已有条目。永久删除是可选操作，
需要 `--delete --yes`；仅 `--delete` 不带 `--yes` 仅切换到预览模式。
`--archive-dir <absolute-path>` 可替换归档根，必须与实时会话树不重叠，
且与 `--delete` 不可同时使用。

`ctx agent session archive <agent> <session> [--archive-dir <absolute-path>]`
对单个符合条件会话应用同样的锁、索引断言、源断言和无替换重命名。
归档后的目录会完整保留原始会话树，包括原始
`messages.jsonl` 和 `events.jsonl`，不做重序列化。

`default`、`index/current` 和显式 `--keep` 名称中的会话，
当 `state` 明确为 `active` 时受到保护。缺失的 `state` 仍兼容旧版会话；
不安全或不可读取的状态条目也要保守保护。GC 只选择活动会话目录，并且从不
选择归档的条目进行第二次操作。`archived_sessions` 是一个
独立主目录，不是新的根 ABI 命名空间。目标冲突或跨文件系统重命名在不删除
活动源的情况下会失败，且不允许递归复制回退。此阶段不定义恢复命令。

会话列表不是根级功能。客户端从会话索引读取当前代理：

```text
/ctx/home/1000/agent/executor/session/index/list
/ctx/home/1000/agent/executor/session/index/current
/ctx/home/1000/agent/executor/session/index/by-cwd/<hash>
/ctx/home/1000/agent/executor/session/index/by-hash/<hash>
/ctx/home/1000/agent/executor/session/index/by-uuid/<uuid>
```

共享会话在 `shared` 下读取匹配索引。临时会话不会出现在
会话列表中。

持久会话不存于 chroot 根目录：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/
```

chroot 根目录只是运行时环境：

```text
/ctx/home/<uid>/agent/<agent>/root/
```

重建根、清理它或切换运行时环境都不应该
销毁会话历史。

上下文窗口限制、可重建的提示工作集以及上下文压缩
规则在 [agent-runtime.md](agent-runtime.md#上下文窗口控制) 中定义。
子进程传递通道及其持久化结果文件定义在
[ctx-coreutils.md](ctx-coreutils.md#核心命令)。
