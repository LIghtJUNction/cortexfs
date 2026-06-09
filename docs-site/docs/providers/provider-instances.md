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

用户 policy 仍然控制 enabled provider 是否对某个 space 可见。
