---
title: First Request
---

# First Request

CortexFS 的文件式提交规则是：普通 `write()` 只写内容，不触发 provider；只有原子 rename 到 `inbox/*.req.json` 才表示提交。

## Submit

```bash
api="$CTX_HOME/api/openai.chat"

printf '%s\n' '{"messages":[{"role":"user","content":"Reply with cortexfs-ok"}]}' \
  > "$api/inbox/001.tmp"

mv "$api/inbox/001.tmp" "$api/inbox/001.req.json"
printf '1\n' > /ctx/control/drain
```

## Read Result

```bash
cat "$api/outbox/001.route.json"
cat "$api/outbox/001.fingerprint"
cat "$api/outbox/001.resp.json"
```

失败时：

```bash
cat "$api/outbox/001.error"
```

## Batch Shell Pattern

```bash
for f in requests/*.json; do
  id="$(basename "$f" .json)"
  cp "$f" "$api/inbox/$id.tmp"
  mv "$api/inbox/$id.tmp" "$api/inbox/$id.req.json"
done

printf '1\n' > /ctx/control/drain
find "$api/outbox" -name '*.resp.json' -print -exec jq . {} \;
```
