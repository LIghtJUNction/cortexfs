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

Create or update provider instances through `provider/inbox`. Do not put API keys
in provider config requests.

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

Read effective URL:

```bash
cat /ctx/provider/openai-main/url/effective
cat /ctx/provider/openai-main/url/source
```

## Enable or disable

Set `enabled` to `true` or `false` in the provider config request and submit it
again.

User policy still controls whether an enabled provider is visible to a space.
