# 终端资源 ABI

本文档定义首个持久终端资源切片。终端不是一次性的 shell 结果，而是由持久会话拥有、由可替换进程、PTY 和实时传输实现的资源。

## 范围与根边界

当前根 ABI 保持冻结。因此，该资源投影在现有 Agent 会话之下：

~~~text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/<terminal-id>/
  meta.json
  state
  status
  owner
  cwd
  events.jsonl
~~~

该路径是会话本地资源，不是新的顶层根分类。未来若要投影
/ctx/terminal，需要单独版本化的 root-ABI 决策，不能静默创建第二套生命周期或提交命名空间。

当前实现会在 Agent 启动时创建一个由 Agent 支持的终端。独立的命令监督器、通用命令创建以及多个独立进程对象留待后续版本。

## 身份与层次

兼容终端的稳定 ID 为：

~~~text
terminal-<agent>-<session>
~~~

该 ID 由 Agent 名称和会话名称派生，不是 socket 路径。各层如下：

~~~text
resource     terminal/<terminal-id>/ 普通文件
instance     一次受监督的启动收据与调用
process      ctxterm 子命令进程
PTY          portable-pty 主从对
attachment   零个或多个 watch/attach 客户端
~~~

会话拥有资源目录。监督器拥有实时进程真相。socket 只是活动 PTY 的定位器；它消失时不应删除元数据或历史。

## 文件

`meta.json` 是完整的可检查记录：

~~~json
{
  "id": "terminal-executor-default",
  "agent": "executor",
  "session": "default",
  "owner": "1000",
  "cwd": "/workspace",
  "command": ["/ctx/bin/tsh"],
  "state": "running",
  "socket": "/run/user/1000/cortexfs/terminal/executor/default/main.sock",
  "created_at": 1735689600
}
~~~

`state` 是规范的短文本投影；`status` 是兼容别名。实现可以使用 `created`、`running`、`exited` 或 `error`。`owner` 和 `cwd` 是记录的文本投影。所有元数据写入都使用普通的“临时文件写入后在同目录原子 rename”规则。

`events.jsonl` 是追加写入的文件，每行一个 JSON 对象。PTY 输出按字节处理，并编码为标准 base64：

~~~json
{"seq":1,"ts":1735689601,"type":"pty.output","data_b64":"Y2FyZ28gYnVpbGQK"}
{"seq":2,"ts":1735689602,"type":"process.exit","exit_code":0}
~~~

同一资源内的 `seq` 单调递增。当前 `type` 只有 `pty.output` 或 `process.exit`。PTY 会按设计合并 stdout 与 stderr；消费者不能从事件类型推断独立输出流。后续 ABI 可以增加显式流标签，但不能改变现有事件的含义。

## 命令

人类 CLI 是该资源之上的轻量客户端：

~~~text
ctx terminal create AGENT [--session SESSION] [--cwd PATH]
ctx terminal list
ctx terminal status TERMINAL
ctx terminal watch TERMINAL
ctx terminal attach TERMINAL
~~~

`create` 准备持久会话资源并启动现有的受监督 Agent 终端。`list` 和 `status` 只读。`watch` 接收 PTY 字节但不写入；`attach` 接收 PTY 字节并转发输入，与现有 Agent 终端 attach 行为一致。`list` 和 `status` 都不会启动运行时或改变进程状态。

## 能力

即使当前 CLI 仍由 Agent 支持，终端权限也按能力划分：

~~~text
terminal.create
terminal.read
terminal.write
terminal.resize
terminal.signal
terminal.stop
terminal.kill
~~~

策略层必须独立授予每项操作。能够读写 PTY 的调用方不会自动获得 signal 或 kill 权限。当前 watch 和 attach 路由复用现有 Agent socket 策略；这些能力名称留给独立终端监督器版本。

## Attach 与生命周期

人类和 Agent 的 attachment 都是终端资源的对等方：

~~~text
terminal resource
  +-- Agent attachment
  +-- human watch
  +-- human attach
  +-- another authorized attachment
~~~

资源的生命周期长于某一个 attachment，并在 PTY 退出后保留事件。Agent teardown 可以停止当前兼容进程，但不会删除会话资源或其事件历史。未来的独立监督器必须将这种生命周期独立性作为默认行为。
