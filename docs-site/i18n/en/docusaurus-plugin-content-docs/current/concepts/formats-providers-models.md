---
title: Formats, Providers, Models
---

# Formats, Providers, Models

CortexFS separates API format, provider instance, and model.

## Format

`format/` describes protocol shape:

```text
format/
  openai.chat/
  openai.responses/
  anthropic.messages/
  google.generate_content/
```

Providers that speak the OpenAI chat shape share `openai.chat`, whether they
are official accounts, relays, Kimi, MiniMax, or local runtimes.

## Provider

A provider is a backend/account instance, not a vendor brand.

```text
provider/
  openai-main/
  openai-relay-a/
  kimi-main/
  minimax-main/
  local-vllm/
```

One vendor can have many provider instances. A relay can also be a provider.

## Model

Global model index:

```text
model/<provider-id>.<model-id>/
  provider
  model
  format
  cap
  status
```

User-visible models live under `home/<uid>/model/`.
