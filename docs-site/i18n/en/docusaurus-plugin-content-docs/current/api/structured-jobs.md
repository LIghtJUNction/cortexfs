---
title: Structured Jobs
---

# Structured Jobs

Structured jobs implement the “write a spec, write a request, read JSON output” workflow. Translation, extraction, classification, and rewriting should share this file ABI.

## Shape

```text
home/<uid>/job/
  count
  list
  <id>/
    spec
    req
    out.json
    status
```

`spec` is the small-text contract. `req` is the request input. After writing `req`, CortexFS generates `out.json` according to `spec`.

## Translation Example

```bash
job="/ctx/home/$(id -u)/job/translate.zh"
mkdir "$job"

cat > "$job/spec" <<'EOF'
kind=translate
from=en
to=zh
out=json
fields=text,from,to,input
EOF

printf 'hello world\n' > "$job/req"
cat "$job/out.json"
cat "$job/status"
```

Output:

```json
{"text":"你好，世界","from":"en","to":"zh","input":"hello world"}
```

The current implementation is synchronous and deterministic. The same ABI will later connect to the worker pool and streaming LLM output without changing user scripts.
