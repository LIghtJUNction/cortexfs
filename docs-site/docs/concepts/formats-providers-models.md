---
title: 协议格式、Provider 和模型
---

# 协议格式、Provider 和模型

CortexFS 把 API format、provider instance 和 model 分成三个层级。

## 协议格式

`format/` 描述请求协议形状，不等于 provider。

```text
format/
  openai.chat/
  openai.responses/
  anthropic.messages/
  google.generate_content/
```

使用 OpenAI 请求格式的 provider 共享 `openai.chat` 或 `openai.responses`。Kimi、MiniMax、中转站、本地模型服务都可以是 `openai.chat` provider。

## Provider

Provider 是后端实例，不是厂商品牌。

```text
provider/
  openai-main/
  openai-relay-a/
  kimi-main/
  minimax-main/
  local-vllm/
```

同一个厂商可以有多个 provider instance；一个中转站也可以是 provider。

## 模型

全局模型索引：

```text
model/<provider-id>.<model-id>/
  provider
  model
  format
  cap
  status
```

用户实际可用模型：

```text
home/<uid>/model/
  count
  list
  <provider-id>.<model-id>/
    allowed
    reason
```

查询某个用户或 agent 能用什么，应该读用户模型视图，而不是只读全局 provider 视图。
