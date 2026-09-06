#!/usr/bin/env bash
set -euo pipefail

ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PIPELINE="$REPO/examples/agent-pipeline.sh"
FAKE_CTX="$ROOT/ctx"
LOG="$ROOT/calls.log"
STATE="$ROOT/state"

cat >"$FAKE_CTX" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_CTX_LOG:?}"
: "${FAKE_CTX_STATE:?}"
case "${1:-}:${2:-}" in
ping:agent/*) exit 0 ;;
cat:agent/*.d/cwd)
  printf '%s\n' '/workspace'
  exit 0
  ;;
cat:agent/*.d/mount)
  if [[ $2 == *"/${FAKE_CTX_WORKSPACE_MISMATCH:-__none__}.d/"* ]]; then
    printf '%s\n' '/different\t/workspace\trw'
  else
    printf '%s\n' '/shared\t/workspace\trw'
  fi
  exit 0
  ;;
esac
printf '%q\t%q\t%q\t%q\t%q\t%q\n' "$1" "$2" "$3" "$4" "$5" "$6" >>"$FAKE_CTX_LOG"
agent=$3
count=0
[[ ! -f $FAKE_CTX_STATE ]] || read -r count <"$FAKE_CTX_STATE"
count=$((count + 1))
printf '%s\n' "$count" >"$FAKE_CTX_STATE"
if [[ ${FAKE_CTX_FAIL_STAGE:-} == "$agent" ]]; then
  exit 72
fi
if [[ ${FAKE_CTX_BIG_STAGE:-} == "$agent" ]]; then
  head -c "${FAKE_CTX_BIG_BYTES:-200}" /dev/zero | tr '\0' x
  exit 0
fi
if [[ $agent == reviewer && -n ${FAKE_CTX_REVIEW:-} ]]; then
  printf '%s\n' "$FAKE_CTX_REVIEW"
  exit 0
fi
case "$agent:$count" in
  architect:1) printf '%s\n' 'PLAN: inspect, edit, test' ;;
  coder:2) printf '%s\n' 'CODE: first implementation' ;;
  reviewer:3) printf '%s\n' 'CHANGES' 'fix the edge case' ;;
  coder:4) printf '%s\n' 'CODE: fixed implementation' ;;
  reviewer:5) printf '%s\n' 'PASS' 'verified independently' ;;
  reviewer:*) printf '%s\n' 'PASS' ;;
  *) printf '%s\n' 'fixture output' ;;
esac
EOF
chmod 0700 "$FAKE_CTX"
export CTX_BIN="$FAKE_CTX" FAKE_CTX_LOG="$LOG" FAKE_CTX_STATE="$STATE"
export CTX_PIPELINE_CODER=coder CTX_PIPELINE_REVIEWER=reviewer

OUTPUT=$("$PIPELINE" --session-prefix smoke --max-rounds 2 -- 'fix the parser')
[[ $OUTPUT == $'PASS\nverified independently' ]]
[[ $(wc -l <"$LOG") -eq 5 ]]
grep -F $'agent\tsend\tarchitect\t--session\tsmoke-plan' "$LOG" >/dev/null
grep -F $'agent\tsend\tcoder\t--session\tsmoke-code' "$LOG" >/dev/null
grep -F $'agent\tsend\treviewer\t--session\tsmoke-review' "$LOG" >/dev/null

grep -F 'Architect plan:' "$LOG" >/dev/null
grep -F 'Previous review:' "$LOG" >/dev/null

: >"$LOG"
printf '0\n' >"$STATE"
set +e
FAKE_CTX_FAIL_STAGE=coder "$PIPELINE" --session-prefix fail -- 'task' >"$ROOT/out" 2>"$ROOT/err"
status=$?
set -e
[[ $status -eq 72 ]]
[[ $(wc -l <"$LOG") -eq 2 ]]
grep -F 'code round 1 failed with status 72' "$ROOT/err" >/dev/null

: >"$LOG"
printf '0\n' >"$STATE"
set +e
CTX_PIPELINE_MAX_STAGE_BYTES=1024 FAKE_CTX_BIG_STAGE=architect \
  FAKE_CTX_BIG_BYTES=1048576 \
  "$PIPELINE" --session-prefix large -- 'task' >"$ROOT/out" 2>"$ROOT/err"
status=$?
set -e
[[ $status -eq 65 ]]
[[ $(wc -l <"$LOG") -eq 1 ]]
[[ $(wc -c <"$ROOT/out") -eq 0 ]]
grep -F 'plan output exceeds 1024 bytes' "$ROOT/err" >/dev/null

: >"$LOG"
printf '0\n' >"$STATE"
if FAKE_CTX_REVIEW=MAYBE "$PIPELINE" --session-prefix verdict -- 'task' >"$ROOT/out" 2>"$ROOT/err"; then
  printf '%s\n' 'expected invalid reviewer verdict' >&2
  exit 1
fi
grep -F 'invalid reviewer verdict: MAYBE' "$ROOT/err" >/dev/null

: >"$LOG"
printf '0\n' >"$STATE"
set +e
FAKE_CTX_WORKSPACE_MISMATCH=reviewer \
  "$PIPELINE" --session-prefix mismatch -- 'task' >"$ROOT/out" 2>"$ROOT/err"
status=$?
set -e
[[ $status -eq 64 ]]
[[ ! -s $LOG ]]
grep -F 'agents do not share one cwd/mount contract' "$ROOT/err" >/dev/null

: >"$LOG"
printf '0\n' >"$STATE"
FAKE_CTX_REVIEW=PASS "$PIPELINE" -- 'task one' >/dev/null
first_session=$(awk -F '\t' '$3 == "architect" {print $5; exit}' "$LOG")
: >"$LOG"
printf '0\n' >"$STATE"
FAKE_CTX_REVIEW=PASS "$PIPELINE" -- 'task two' >/dev/null
second_session=$(awk -F '\t' '$3 == "architect" {print $5; exit}' "$LOG")
[[ $first_session != "$second_session" ]]
[[ $first_session == pipeline-*-plan ]]
[[ $second_session == pipeline-*-plan ]]

printf '%s\n' 'agent pipeline smoke: ok'
