---
title: Provider/Route 替代 Channel
---

# Provider/Route 替代 Channel

CortexFS 不再提供 `/ctx/chan`。之前的 channel 概念拆成两个已有文件对象：

```text
provider/<id>/          一个后端实例：URL、账号类型、格式、健康、模型、secret 状态
home/<uid>/route/<fmt>/ 当前用户对某个 API format 的路由结果
```

这样不会在 provider 和 route 之外再造一套“渠道”抽象。

## 配置后端实例

```bash
cat /ctx/provider/list
cat /ctx/provider/openai-main/format
cat /ctx/provider/openai-main/url/effective
cat /ctx/provider/openai-main/secrets/status
```

开发期可以通过 provider 的小文本控制节点调整运行态视图：

```bash
printf 'https://relay.example.com/v1\n' > /ctx/provider/openai-main/url/current
printf '1\n' > /ctx/provider/openai-main/enabled/current
```

真实 key 不进入挂载树；挂载树只暴露 secret 状态和 key id。

## 观察路由

```bash
CTX_HOME=/ctx/home/$(id -u)
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
```

## 提交请求

所有外部网关、workflow、bot bridge 都走统一文件提交：

```bash
api="$CTX_HOME/api/openai.chat"
printf '%s\n' '{"messages":[{"role":"user","content":"hello"}]}' > "$api/inbox/001.tmp"
mv "$api/inbox/001.tmp" "$api/inbox/001.req.json"
cat "$api/outbox/001.fingerprint"
```

CortexFS 的本地 HTTP/UDS 入口也必须进入同一条 provider、route、policy、store、audit 管线，不能绕过文件 ABI 产生另一套事实。
