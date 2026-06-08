---
title: File Types
---

# File Types

```text
no extension    small text attribute or control node
*.req.json      native API request
*.resp.json     native API response
*.error         error object
*.jsonl         append-only log/message/audit/export
*.md            human-readable view
*.sock          Unix domain socket fast path
schema.json     large schema
manifest.json   large manifest
```

Errors:

```text
invalid write       EINVAL
permission denied   EACCES
read-only write     EROFS
unsupported         ENOSYS
```

Submission:

```bash
printf '%s\n' "$json" > "$inbox/001.tmp"
mv "$inbox/001.tmp" "$inbox/001.req.json"
```
