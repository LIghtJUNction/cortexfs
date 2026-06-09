---
title: 文件 API
---

# 文件 API

每个用户工作入口按 format 暴露 API。

```text
home/<uid>/api/
  openai.chat/
    inbox/
    outbox/
  openai.responses/
    inbox/
    outbox/
  anthropic.messages/
    inbox/
    outbox/
  google.generate_content/
    inbox/
    outbox/
```

## 提交契约

- `write()` 不触发 API。
- 同目录 rename 到 `inbox/*.req.json` 才触发提交。
- rename 只入队并写派生事实，不在 FUSE 回调里调用远程 provider。
- request id 来自文件名 stem。
- 请求和响应保持原生 API JSON。
- 每次请求都计算 fingerprint。
- 每次请求都写 audit。

## Outbox 文件

```text
outbox/<id>.route.json
outbox/<id>.fingerprint
outbox/<id>.resp.json
outbox/<id>.error
```

`route.json` 暴露 provider、model、format、reason 和 fingerprint 等 route metadata。

不存在 `job/`、`hook/`、`chan/` 的第二套提交语义；外部任务和触发器都应写入这些通用 inbox。
