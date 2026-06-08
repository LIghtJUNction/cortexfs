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
```

这些文件只暴露状态，不暴露 secret 值。

## 规则

- API key、OAuth token 和 session token 不是 FUSE 中可写的明文文件。
- `active` 是 key id 或 secret reference，不是原始 key。
- `rotate` 是 daemon 侧轮转控制节点。
- `cortexd` 从受保护的 secret store 解析真实 secret。
- Audit 必须打码 secret material。

## 轮转

```bash
printf '1\n' > /ctx/provider/openai-main/secrets/rotate
cat /ctx/provider/openai-main/secrets/status
cat /ctx/provider/openai-main/secrets/active
```
