---
title: Provider Instances
---

# Provider Instances

A provider is a backend instance, not a vendor brand and not a `/ctx/chan` compatibility alias. Multiple `url + secret ref + format/model` combinations should be represented as multiple `provider/<id>` entries. Route selection is exposed under `home/<uid>/route/<format>/`.

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

## Configure URL

```bash
printf 'https://api.openai.com/v1\n' > /ctx/provider/openai-main/url/current
printf 'https://relay.example.com/v1\n' > /ctx/provider/relay-openai/url/current
```

Read effective URL:

```bash
cat /ctx/provider/openai-main/url/effective
cat /ctx/provider/openai-main/url/source
```

## Enable or disable

```bash
printf '1\n' > /ctx/provider/openai-main/enabled/current
printf '0\n' > /ctx/provider/relay-openai/enabled/current
```

User policy still controls whether an enabled provider is visible to a space.
