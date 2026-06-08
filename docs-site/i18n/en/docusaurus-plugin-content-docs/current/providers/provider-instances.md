---
title: Provider Instances
---

# Provider Instances

Multiple `base_url + key` pairs should be represented as multiple provider
instances.

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
  priority
  health/
  secrets/
  model/
```

Configure URLs:

```bash
printf 'https://api.openai.com/v1\n' > /ctx/provider/openai-main/url/current
printf 'https://relay.example.com/v1\n' > /ctx/provider/relay-openai/url/current
```

Read effective URL:

```bash
cat /ctx/provider/openai-main/url/effective
cat /ctx/provider/openai-main/url/source
```
