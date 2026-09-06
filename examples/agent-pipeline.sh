#!/usr/bin/env bash
set -euo pipefail

CTX_BIN=${CTX_BIN:-ctx}
ARCHITECT=${CTX_PIPELINE_ARCHITECT:-architect}
CODER=${CTX_PIPELINE_CODER:-executor}
REVIEWER=${CTX_PIPELINE_REVIEWER:-product-manager}
SESSION_PREFIX=${CTX_PIPELINE_SESSION_PREFIX:-pipeline-$(date -u +%Y%m%dT%H%M%S)-$BASHPID}
MAX_ROUNDS=${CTX_PIPELINE_MAX_ROUNDS:-1}
MAX_STAGE_BYTES=${CTX_PIPELINE_MAX_STAGE_BYTES:-65536}

usage() {
  cat <<'EOF'
usage: agent-pipeline.sh [options] -- TASK...

Options:
  --architect NAME       planning agent (default: architect)
  --coder NAME           implementation agent (default: executor)
  --reviewer NAME        review agent (default: product-manager)
  --session-prefix NAME  durable session prefix (an existing prefix resumes it)
  --max-rounds N         coder/reviewer rounds, 1..3 (default: 1)

The script composes existing `ctx agent send` calls. It creates no workflow ABI.
All role listeners must expose one identical non-empty cwd/mount contract.
The final reviewer line must begin with PASS or CHANGES.
EOF
}

die() {
  printf 'agent-pipeline: %s\n' "$*" >&2
  exit 64
}

while (($#)); do
  case "$1" in
  --architect)
    (($# >= 2)) || die '--architect requires a value'
    ARCHITECT=$2
    shift 2
    ;;
  --coder)
    (($# >= 2)) || die '--coder requires a value'
    CODER=$2
    shift 2
    ;;
  --reviewer)
    (($# >= 2)) || die '--reviewer requires a value'
    REVIEWER=$2
    shift 2
    ;;
  --session-prefix)
    (($# >= 2)) || die '--session-prefix requires a value'
    SESSION_PREFIX=$2
    shift 2
    ;;
  --max-rounds)
    (($# >= 2)) || die '--max-rounds requires a value'
    MAX_ROUNDS=$2
    shift 2
    ;;
  --help | -h)
    usage
    exit 0
    ;;
  --)
    shift
    break
    ;;
  -*) die "unknown option: $1" ;;
  *) break ;;
  esac
done

(($#)) || die 'TASK is required'
TASK=$*
[[ $SESSION_PREFIX =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,47}$ ]] ||
  die 'session prefix must be 1..48 safe characters'
[[ $MAX_ROUNDS =~ ^[1-3]$ ]] || die 'max rounds must be 1, 2, or 3'
[[ $MAX_STAGE_BYTES =~ ^[1-9][0-9]*$ ]] || die 'stage byte limit must be positive'
((MAX_STAGE_BYTES <= 16777216)) || die 'stage byte limit must not exceed 16 MiB'
command -v "$CTX_BIN" >/dev/null 2>&1 || die "ctx binary not found: $CTX_BIN"

bounded() {
  local label=$1 value=$2 bytes
  bytes=$(LC_ALL=C printf '%s' "$value" | wc -c)
  ((bytes <= MAX_STAGE_BYTES)) || {
    printf 'agent-pipeline: %s output is %s bytes; limit is %s (nothing truncated)\n' \
      "$label" "$bytes" "$MAX_STAGE_BYTES" >&2
    return 65
  }
}

bounded task "$TASK"

workspace_contract() {
  local agent=$1 cwd mounts
  "$CTX_BIN" ping "agent/$agent" >/dev/null ||
    die "agent is unavailable: $agent"
  cwd=$("$CTX_BIN" cat "agent/$agent.d/cwd") ||
    die "cannot read cwd control for $agent"
  mounts=$("$CTX_BIN" cat "agent/$agent.d/mount") ||
    die "cannot read mount control for $agent"
  bounded "$agent cwd" "$cwd"
  bounded "$agent mount" "$mounts"
  [[ -n $cwd && -n $mounts ]] ||
    die "agent $agent has no explicit workspace cwd/mount contract"
  printf '%s\n%s' "$cwd" "$mounts"
}

WORKSPACE_CONTRACT=''
for role in "$ARCHITECT" "$CODER" "$REVIEWER"; do
  contract=$(workspace_contract "$role")
  if [[ -z $WORKSPACE_CONTRACT ]]; then
    WORKSPACE_CONTRACT=$contract
  elif [[ $contract != "$WORKSPACE_CONTRACT" ]]; then
    die "agents do not share one cwd/mount contract: $ARCHITECT $CODER $REVIEWER"
  fi
done

PIPELINE_TMP=$(mktemp -d "${TMPDIR:-/tmp}/ctx-agent-pipeline.XXXXXX")
trap 'rm -rf "$PIPELINE_TMP"' EXIT

send() {
  local stage=$1 agent=$2 session=$3 prompt=$4 output_file status sink_status bytes
  local -a pipe_status
  output_file=$(mktemp "$PIPELINE_TMP/stage.XXXXXX")
  printf 'agent-pipeline: %s -> %s (session %s)\n' "$stage" "$agent" "$session" >&2
  set +e
  "$CTX_BIN" agent send "$agent" --session "$session" "$prompt" |
    {
      head -c "$((MAX_STAGE_BYTES + 1))" >"$output_file"
      head_status=$?
      cat >/dev/null
      cat_status=$?
      ((head_status == 0 && cat_status == 0))
    }
  pipe_status=("${PIPESTATUS[@]}")
  set -e
  status=${pipe_status[0]}
  sink_status=${pipe_status[1]}
  if ((status != 0)); then
    rm -f "$output_file"
    printf 'agent-pipeline: %s failed with status %s\n' "$stage" "$status" >&2
    return "$status"
  fi
  if ((sink_status != 0)); then
    rm -f "$output_file"
    printf 'agent-pipeline: %s bounded output sink failed with status %s\n' \
      "$stage" "$sink_status" >&2
    return 74
  fi
  bytes=$(wc -c <"$output_file")
  if ((bytes > MAX_STAGE_BYTES)); then
    rm -f "$output_file"
    printf 'agent-pipeline: %s output exceeds %s bytes (stream drained; no partial stage used)\n' \
      "$stage" "$MAX_STAGE_BYTES" >&2
    return 65
  fi
  cat "$output_file"
  rm -f "$output_file"
}

PLAN=$(send plan "$ARCHITECT" "$SESSION_PREFIX-plan" \
  "Plan this task for another coding agent. State constraints, files, tests, and acceptance criteria. Do not edit. Task: $TASK")

REVIEW=''
for ((round = 1; round <= MAX_ROUNDS; round++)); do
  if ((round == 1)); then
    CODER_PROMPT="Implement the task in the current workspace. Follow the plan, run only permitted checks, and report changed files plus evidence. Task: $TASK

Architect plan:
$PLAN"
  else
    CODER_PROMPT="Revise the implementation to address the review. Preserve correct work and report new evidence. Task: $TASK

Architect plan:
$PLAN

Previous review:
$REVIEW"
  fi
  CODER_RESULT=$(send "code round $round" "$CODER" "$SESSION_PREFIX-code" "$CODER_PROMPT")
  REVIEW=$(send "review round $round" "$REVIEWER" "$SESSION_PREFIX-review" \
    "Independently inspect the current workspace for this task. Verify claims and tests. Your first non-empty line must be exactly PASS or CHANGES, followed by concise findings. Task: $TASK

Architect plan:
$PLAN

Coder report:
$CODER_RESULT")
  VERDICT=$(printf '%s\n' "$REVIEW" | awk 'NF { print; exit }')
  case "$VERDICT" in
  PASS)
    printf '%s\n' "$REVIEW"
    exit 0
    ;;
  CHANGES) ((round < MAX_ROUNDS)) || {
    printf '%s\n' "$REVIEW"
    exit 1
  } ;;
  *)
    printf 'agent-pipeline: invalid reviewer verdict: %s\n' "${VERDICT:-<empty>}" >&2
    exit 65
    ;;
  esac
done
