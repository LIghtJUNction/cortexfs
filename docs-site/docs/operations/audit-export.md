---
title: 审计和导出
---

# 审计和导出

CortexFS 要求请求、拒绝、执行、错误、route、policy 和导出都可观察。

## 全局审计

```bash
cat /ctx/audit/fields
cat /ctx/audit/events.jsonl
cat /ctx/audit/usage
cat /ctx/audit/cost
```

Audit event 应包含 request id、fingerprint、route metadata、policy decision、provider、model、subject 和 time 等信息。

## 用户导出

```bash
exports="$CTX_HOME/export"

cat "$exports/conversations.jsonl"
cat "$exports/sft.jsonl"
cat "$exports/preference.jsonl"
cat "$exports/tool_calls.jsonl"
cat "$exports/agent_traces.jsonl"
```

## 过滤器

```bash
printf 'helper\n' > "$exports/filter/agent"
printf 'home/1000\n' > "$exports/filter/space"
printf '2\n' > "$exports/filter/from"
cat "$exports/conversations.jsonl"
```

过滤节点包括 provider、model、agent、subject、space、from、to 和 exclude_failed。
