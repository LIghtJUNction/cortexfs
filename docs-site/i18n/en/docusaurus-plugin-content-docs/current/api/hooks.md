---
title: Timers and Hooks
---

# Timers and Hooks

CortexFS does not add background polling, hot reload, or an internal scheduler. Recommended split:

```text
systemd timer / cron / git hook / CI   trigger
CortexFS hook file ABI                 input, output, status, log, audit
cortexd                                future async execution plane
```

## Shape

```text
home/<uid>/hook/
  count
  list
  <id>/
    trigger
    spec
    req
    out.json
    status
    last
    log.jsonl
```

`trigger` declares the external trigger source, for example `manual`, `systemd.timer`, `cron`, or `git.pre-commit`. CortexFS does not start a watcher because of it.

`spec` describes what to do. The first implementation supports binding a hook to a structured job:

```text
kind=job
job=translate.zh
from=en
to=zh
fields=text,from,to,input
```

Writing `req` generates `out.json` and updates `status`, `last`, and `log.jsonl`.

## Example: systemd timer translation

Create the hook:

```bash
hook="/ctx/home/$(id -u)/hook/daily-translate"
mkdir "$hook"

printf 'systemd.timer\n' > "$hook/trigger"
cat > "$hook/spec" <<'EOF'
kind=job
job=translate.zh
from=en
to=zh
fields=text,from,to,input
EOF
```

The external timer only writes the request:

```bash
systemd-run --user --on-calendar='daily' \
  sh -c 'cat ~/todo.txt > /ctx/home/$(id -u)/hook/daily-translate/req'
```

Read the result:

```bash
cat "$hook/out.json"
cat "$hook/status"
cat "$hook/log.jsonl"
```

## Persistence

The hook `trigger` and `spec` are written through to:

```text
~/.config/cortexfs/hook.d/<id>.conf
```

They are restored when the mount starts. `req`, `out.json`, `status`, `last`, and `log.jsonl` are runtime projections.
