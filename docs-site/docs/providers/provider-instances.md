---
title: Provider 实例
---

# Provider 实例

多个 `base_url + key` 组合应该表示为多个 provider instance。

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
  auth_scheme
  account_type
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

```bash
printf 'https://api.openai.com/v1\n' > /ctx/provider/openai-main/url/current
printf 'https://relay.example.com/v1\n' > /ctx/provider/relay-openai/url/current
```

读取 effective URL：

```bash
cat /ctx/provider/openai-main/url/effective
cat /ctx/provider/openai-main/url/source
```

## 启用或禁用

```bash
printf '1\n' > /ctx/provider/openai-main/enabled/current
printf '0\n' > /ctx/provider/relay-openai/enabled/current
```

User policy still controls whether an enabled provider is visible to a space.
