---
title: 任务提交
---

# 任务提交

CortexFS 不提供 `home/<uid>/job` 目录，也不提供内置 job DSL。任务只是一个请求文件：外部程序把任务规范放进 JSON，然后通过通用 inbox/outbox 提交。

统一规则：

```text
write tmp file
rename tmp -> <id>.req.json
read outbox/<id>.resp.json 或 outbox/<id>.error
read audit/events.jsonl
```

## 示例

```bash
CTX_HOME="/ctx/home/$(id -u)"
api="$CTX_HOME/api/openai.chat"

cat > "$api/inbox/translate-001.tmp" <<'JSON'
{"messages":[{"role":"user","content":"Translate to zh-CN: hello world"}]}
JSON

mv "$api/inbox/translate-001.tmp" "$api/inbox/translate-001.req.json"
printf '1\n' > /ctx/control/drain
cat "$api/outbox/translate-001.resp.json"
```

如果 workflow engine 需要 run id、step id、输入来源或重试策略，应把这些信息写入请求 JSON、thread metadata 或自己的状态库；CortexFS 只负责 provider、route、policy、queue、outbox、audit 和 export。
