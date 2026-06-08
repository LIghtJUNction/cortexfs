---
title: Threads and Batch
---

# Threads and Batch

Thread 是持续上下文，Batch 是批处理队列。

## Thread

```text
home/<uid>/thread/<id>/
  inbox/
  io.sock
  messages.jsonl
  latest.md
  state
  fingerprint
  control/
```

提交 thread 请求：

```bash
thread="$CTX_HOME/thread/demo"
printf '%s\n' '{"messages":[{"role":"user","content":"continue"}]}' > "$thread/inbox/0001.tmp"
mv "$thread/inbox/0001.tmp" "$thread/inbox/0001.req.json"
```

Socket fast path 必须和文件式提交进入同一 policy、route、store、audit 和 export 管线。

## Batch

批处理用于多请求入队、drain 和审计。它应满足：

- request id 幂等。
- 并发安全。
- 每个请求都有 route metadata。
- 失败不能吞掉错误对象。
- 导出可以保留 batch 维度。
