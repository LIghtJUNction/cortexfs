---
title: Provider 实例
---

# Provider 实例

Provider 是后端实例，不是厂商品牌，也不是 `/ctx/chan` 的兼容别名。多个 `url + secret ref + format/model` 组合应表示为多个 `provider/<id>`；路由选择由 `home/<uid>/route/<format>/` 暴露。

```text
provider/<id>/
  family
  name
  format
  url/
    default
    current
    effective
    source
  auth
  acct
  enabled/
    default
    current
    effective
    source
  priority
  health/
  secrets/
  model/
```

## 配置 URL

新增或更新 provider instance 走 `provider/inbox`，而不是把 API key 写进配置文件。

```bash
provider=/ctx/provider
cat > "$provider/inbox/relay-openai.tmp" <<'JSON'
{
  "op": "upsert",
  "id": "relay-openai",
  "family": "openai-compatible",
  "name": "Relay endpoint using OpenAI formats",
  "formats": ["openai.chat", "openai.responses"],
  "base_url": "https://relay.example.com/",
  "default_model": "gpt-4o-mini",
  "priority": 80,
  "enabled": true
}
JSON
mv "$provider/inbox/relay-openai.tmp" "$provider/inbox/relay-openai.req.json"
cat "$provider/outbox/relay-openai.resp.json"
```

读取 effective URL：

```bash
cat /ctx/provider/openai-main/url/effective
cat /ctx/provider/openai-main/url/source
```

## 启用或禁用

把 provider config request 里的 `enabled` 设为 `true` 或 `false` 后再次提交即可。

用户 policy 仍然控制 enabled provider 是否对某个 space 可见。
