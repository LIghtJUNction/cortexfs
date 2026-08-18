# 聊天界面 ABI

`ctxchat` 是参考终端界面。它与运行时是分开的：它
读取记录的`/ctx`文件并与...交换以换行符分隔的JSON
`/ctx/agent/<agent>.sock`。UI 设置和主题应存放在普通文件中或
环境变量；运行时不需要 UI 数据库。

`/ctx/agent/<agent>.sock` 可能是所有者授权的运行时符号链接或直接链接
根据部署情况的套接字节点。在假设有一个之前探测挂载的树
实现形式；使用 `readlink -f`/`nc -U` 作为运行时敏感探针。

耐用的历史存在于下方
`/ctx/home/<uid>/agent/<agent>/session/<session>/`: `messages.jsonl` 包含
消息，`events.jsonl` 包含事件，`latest.md` 是一个便捷视图，
并且`state`是终止状态。

最小化 Bash 客户端：

```bash
printf '%s\n' '{"op":"send","id":"ui-1","session":"default","input":"hello"}' |
  socat - UNIX-CONNECT:/ctx/agent/coder.sock
printf '%s\n' '{"op":"tsh","id":"ui-tool-1","session":"default","args":["load","bash"]}' |
  socat - UNIX-CONNECT:/ctx/agent/coder.sock
```

最小化 Python 客户端：

```python
import json, socket
s = socket.socket(socket.AF_UNIX)
s.connect("/ctx/agent/coder.sock")
s.sendall((json.dumps({"op": "send", "id": "py-1", "session": "default",
                       "input": "hello"}) + "\n").encode())
s.shutdown(socket.SHUT_WR)
for line in s.makefile():
    frame = json.loads(line)
    print(frame)
    if frame.get("type") == "done":
        break
```

`ctxchat` 增加了自动完成、多行粘贴、`:tsh` 命令、有限的 `@path`
以及`@history:N`引用和剪贴板适配器。引用是提示
仅限上下文，绝不授予文件系统或
工具权限。
