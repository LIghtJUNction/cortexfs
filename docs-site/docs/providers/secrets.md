---
title: 密钥
---

# 密钥

Provider secret 不进入挂载树。

## 暴露文件

```text
provider/<id>/secrets/
  status
  active
  rotate
  last_rotated
  next_rotation
  inbox/
  outbox/
```

这些文件只暴露状态，不暴露 secret 值。

## 规则

- API key、OAuth token 和 session token 不是 FUSE 中可写的明文文件。
- `active` 是 key id 或 secret reference，不是原始 key。
- `rotate` 是 daemon 侧轮转控制节点。
- `cortexd` 从受保护的 secret store 解析真实 secret。
- Audit 必须打码 secret material。

## 导入 API key

导入使用同目录原子 rename。请求体会被处理进系统 secret store；挂载树只留下状态和 secret reference。

```bash
secrets=/ctx/provider/relay-openai/secrets
cat > "$secrets/inbox/api-key.tmp" <<'JSON'
{
  "op": "import",
  "kind": "bearer",
  "value": "sk-placeholder"
}
JSON
mv "$secrets/inbox/api-key.tmp" "$secrets/inbox/api-key.req.json"
cat "$secrets/outbox/api-key.resp.json"
cat "$secrets/status"
cat "$secrets/active"
```

## 轮转

```bash
printf '1\n' > /ctx/provider/openai-main/secrets/rotate
cat /ctx/provider/openai-main/secrets/status
cat /ctx/provider/openai-main/secrets/active
```
