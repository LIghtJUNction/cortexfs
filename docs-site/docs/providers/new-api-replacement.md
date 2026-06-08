---
title: 替代 New API
---

# 替代 New API

CortexFS 的目标不是做 provider demo，而是替代 New API 这一类本地网关控制面。

默认状态必须是零后端：

```text
/ctx/provider/list   空
/ctx/model/list      空
```

只有用户显式配置后才出现渠道、模型和路由。

## 本地统一入口

对外只暴露一个本地标准入口：

```text
http://127.0.0.1:6185/v1
```

兼容入口：

```text
GET  /v1/models
POST /v1/chat/completions
POST /v1/responses
POST /v1/messages
POST /v1/generateContent
```

所有入口进入同一管线：

```text
parse
norm
route
policy
key
send
store
log
bill
```

不能存在绕过 policy、key、log、bill 的旁路。

## 短命名

对外 ABI 使用短词，避免下划线：

```text
chan     渠道，一个 url + keyref + fmt + model 集合
url      上游地址
keyref   密钥引用，不是明文 key
fmt      协议格式
mod      模型名
grp      用户组
tok      本地访问令牌
quota    额度
ratio    计费倍率
prio     fallback 优先级
wt       同优先级权重
fb       fallback 策略
log      请求日志
```

## 控制面

文件 ABI 应映射到这些 New API 等价能力：

```text
chan/
  count
  list
  <id>/
    url
    fmt
    keyref
    mod/
    grp
    ratio
    prio
    wt
    state
    health/

tok/
  count
  list
  <id>/
    name
    grp
    quota
    state

route/
  openai.chat/
    fb
    chan
    mod
    why

log/
  req.jsonl
  bill.jsonl
```

旧的 `provider/` 可以保留为兼容视图，但核心控制面应该收敛到 `chan/`。

## Fallback

`fb` 是可配置策略，不是硬编码分支。

基础策略：

```text
prio    先选最高可用优先级
wt      同优先级按权重分流
health  跳过 down、限流、超额、缺 key 的 chan
retry   按策略重试同级或下级
```

例子：

```text
chan/openai-a prio=100 wt=80
chan/openai-b prio=100 wt=20
chan/kimi-a   prio=80  wt=100
```

## 持久化

写入 `/ctx` 不能只是内存态。配置应落到本地持久存储：

```text
~/.config/cortexfs/chan.d/*.conf
~/.config/cortexfs/tok.d/*.conf
```

key 不落明文，只保存 `keyref`。真实 secret 由 keyring、pass、sops 或 daemon secret store 解析。
