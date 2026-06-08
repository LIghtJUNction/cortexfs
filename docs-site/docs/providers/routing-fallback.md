---
title: 路由和 Fallback
---

# 路由和 Fallback

使用 `prio` 做 fallback 层级，使用 `wt` 做同层流量分配。

```text
openai-a   prio=100 wt=80
openai-b   prio=100 wt=20
kimi-a     prio=80  wt=100
local-a    prio=10  wt=100
```

## 语义

1. 先尝试最高健康 `prio` 层。
2. 同层按 `wt` 分流。
3. 只有当前层不可用、禁用、限流、缺 key 或熔断时，才降级到低层。
4. 每次选择都必须写入 route metadata 和 audit/log。

## 读取当前路由

```bash
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
```

读取选中 provider 的 URL：

```bash
p=$(cat "$CTX_HOME/route/openai.chat/provider")
cat "/ctx/provider/$p/url/effective"
```

目标实现必须由 `cortexd` 执行可配置 fallback，同时保留同一条 parse/norm/route/policy/key/send/store/log/bill 管线。
