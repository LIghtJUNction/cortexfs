---
title: Local API
---

# Local API

CortexFS 预留本地统一 API，使外部 OpenAI-compatible 客户端可以接入同一执行面。

```text
127.0.0.1:6185
/run/user/<uid>/cortex/api.sock
```

HTTP endpoint：

```text
GET  /v1/models
POST /v1/chat/completions
POST /v1/responses
POST /v1/messages
POST /v1/generateContent
```

## Discovery

```bash
cat "$CTX_HOME/api/endpoints"
cat "$CTX_HOME/api/http/listen"
cat "$CTX_HOME/api/unix/path"
cat "$CTX_HOME/api/pipeline"
```

## Pipeline

文件路径、HTTP 和 Unix socket 必须进入同一内部管线：

```text
normalize format
route
policy check
secret resolve
provider call
store response
append thread if bound
audit
```

不能存在不审计、不受 policy 控制、不写 store 的旁路。
