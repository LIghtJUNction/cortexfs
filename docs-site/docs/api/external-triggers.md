---
title: 外部触发器
---

# 外部触发器

CortexFS 不提供 `home/<uid>/hook` 目录，也不把外部触发器命名为文件系统 ABI。systemd timer、cron、git hook、CI、webhook bridge 等外部触发器应直接写通用 inbox，并用原子 rename 提交。

推荐分工：

```text
systemd timer / cron / CI / webhook   决定何时触发
CortexFS inbox/outbox                  接收请求、暴露结果
cortexd / control/drain                执行队列
CortexFS audit                         记录事实
```

## systemd timer 示例

```bash
systemd-run --user --on-calendar='daily' \
  sh -lc 'CTX_HOME=/ctx/home/$(id -u); api=$CTX_HOME/api/openai.chat; id=daily-$(date +%Y%m%d); printf %s "{\"messages\":[{\"role\":\"user\",\"content\":\"Summarize ~/todo.txt\"}]}" > "$api/inbox/$id.tmp"; mv "$api/inbox/$id.tmp" "$api/inbox/$id.req.json"'
```

读取结果仍然是：

```bash
cat /ctx/home/$(id -u)/api/openai.chat/outbox/<id>.resp.json
cat /ctx/audit/events.jsonl
```
