#!/usr/bin/env bash
set -euo pipefail

PORT=${AIMOCK_PORT:-4010}
BASE_URL="http://127.0.0.1:${PORT}"
LOG=${TMPDIR:-/tmp}/cortexfs-aimock-smoke.$$.log

cleanup() {
  if [[ -n ${SERVER_PID:-} ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -f "$LOG"
}
trap cleanup EXIT

npm run aimock -- --port "$PORT" --log-level warn >"$LOG" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 50); do
  if curl -fsS "${BASE_URL}/health" >/dev/null 2>&1 ||
     curl -fsS "${BASE_URL}/__aimock/health" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    cat "$LOG" >&2
    exit 1
  fi
  sleep 0.1
done

response=$(curl -fsS "${BASE_URL}/v1/chat/completions" \
  -H 'Authorization: Bearer mock' \
  -H 'Content-Type: application/json' \
  -d '{"model":"gpt-5.6","messages":[{"role":"user","content":"hi"}],"stream":false}')

case "$response" in
  *"cortexfs aimock ok"*) ;;
  *)
    printf 'unexpected aimock response:\n%s\n' "$response" >&2
    cat "$LOG" >&2
    exit 1
    ;;
esac

stream=$(curl -fsS "${BASE_URL}/v1/chat/completions" \
  -H 'Authorization: Bearer mock' \
  -H 'Content-Type: application/json' \
  -d '{"model":"gpt-5.6","messages":[{"role":"user","content":"hi"}],"stream":true}')

case "$stream" in
  *"data:"*"cortexfs aimock ok"*"[DONE]"*) ;;
  *)
    printf 'unexpected aimock stream:\n%s\n' "$stream" >&2
    cat "$LOG" >&2
    exit 1
    ;;
esac

printf 'aimock smoke passed: %s/v1\n' "$BASE_URL"
