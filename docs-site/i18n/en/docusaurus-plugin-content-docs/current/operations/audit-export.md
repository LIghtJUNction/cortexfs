---
title: Audit and Export
---

# Audit and Export

CortexFS requires requests, denials, execution, errors, routing, policy, and
exports to be inspectable.

Global audit:

```bash
cat /ctx/audit/fields
cat /ctx/audit/events.jsonl
cat /ctx/audit/usage
cat /ctx/audit/cost
```

User exports:

```bash
exports="$CTX_HOME/export"

cat "$exports/conversations.jsonl"
cat "$exports/sft.jsonl"
cat "$exports/preference.jsonl"
cat "$exports/tool_calls.jsonl"
cat "$exports/agent_traces.jsonl"
```

Filters:

```bash
printf 'helper\n' > "$exports/filter/agent"
printf 'home/1000\n' > "$exports/filter/space"
printf '2\n' > "$exports/filter/from"
cat "$exports/conversations.jsonl"
```
