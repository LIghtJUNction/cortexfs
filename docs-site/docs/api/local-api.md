---
title: 本地 API
---

# 本地 API

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

## 发现

```bash
cat "$CTX_HOME/api/endpoints"
cat "$CTX_HOME/api/http/listen"
cat "$CTX_HOME/api/http/localurl"
cat "$CTX_HOME/api/unix/path"
cat "$CTX_HOME/api/pipeline"
```

`localurl` 是客户端应该读取的本地 OpenAI-compatible base URL：

```bash
base_url="$(cat "$CTX_HOME/api/http/localurl")"
```

当前投影值是 `http://127.0.0.1:6185/v1`。这是发现 ABI；实际 HTTP listener 属于后续 daemon 运行面，不要把它和 FUSE 文件投影混为一谈。

## 管线

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
