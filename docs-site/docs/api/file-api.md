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
- rename 到 `inbox/*.req.json` 才触发提交。
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
