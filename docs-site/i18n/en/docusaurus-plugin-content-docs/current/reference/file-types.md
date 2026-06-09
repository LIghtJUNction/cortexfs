---
title: File Types
---

# File Types

```text
no extension    small text attribute or control node
*.req.json      native API request
*.resp.json     native API response
*.error         error object
*.jsonl         append-only logs, messages, audit, training data
*.md            human-readable view
*.sock          Unix domain socket fast path
schema.json     large structure schema
manifest.json   large structure manifest
```

Small text files use one value per file. Multi-value files use one value per
line, booleans use `0` or `1`, and reads include a trailing newline.
