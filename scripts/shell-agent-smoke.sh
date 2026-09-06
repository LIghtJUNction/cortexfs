#!/usr/bin/env bash
set -euo pipefail

ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
AGENT="$REPO/examples/extensions/shell-agent/agent.sh"
ENVELOPE='{"schema":"cortexfs.agent-invocation/v1","run":"run-1","step":0,"input":"record this","event":null,"origin":null,"history_messages":"","tool_context":"","observation":null}'

OUTPUT=$(printf '%s\n' "$ENVELOPE" | env \
  CTX_AGENT_LAUNCH=sdk-envelope-v1 \
  CTX_RUN_ID=run-1 \
  CTX_AGENT_STEP=0 \
  CTX_SESSION=demo \
  SHELL_AGENT_PREFIX=Notebook \
  "$AGENT" --cortexfs-sdk-envelope-v1)
[[ $(printf '%s' "$OUTPUT" | jq -r '.type') == message ]]
[[ $(printf '%s' "$OUTPUT" | jq -r '.run') == run-1 ]]
[[ $(printf '%s' "$OUTPUT" | jq -r '.content[0].text') == 'Notebook[demo]: record this' ]]

if printf '%s\n' "$ENVELOPE" | env \
  CTX_AGENT_LAUNCH=wrong CTX_RUN_ID=run-1 CTX_AGENT_STEP=0 \
  "$AGENT" --cortexfs-sdk-envelope-v1 >/dev/null 2>&1; then
  printf '%s\n' 'expected launch marker rejection' >&2
  exit 1
fi
if printf '%s\n%s\n' "$ENVELOPE" "$ENVELOPE" | env \
  CTX_AGENT_LAUNCH=sdk-envelope-v1 CTX_RUN_ID=run-1 CTX_AGENT_STEP=0 \
  "$AGENT" --cortexfs-sdk-envelope-v1 >/dev/null 2>&1; then
  printf '%s\n' 'expected sticky-frame rejection' >&2
  exit 1
fi
if printf '%s\n' "$ENVELOPE" | env \
  CTX_AGENT_LAUNCH=sdk-envelope-v1 CTX_RUN_ID=run-2 CTX_AGENT_STEP=0 \
  "$AGENT" --cortexfs-sdk-envelope-v1 >/dev/null 2>&1; then
  printf '%s\n' 'expected run mismatch rejection' >&2
  exit 1
fi
if printf '%s\n' "$ENVELOPE" | env \
  CTX_AGENT_LAUNCH=sdk-envelope-v1 CTX_RUN_ID=run-1 CTX_AGENT_STEP=1 \
  "$AGENT" --cortexfs-sdk-envelope-v1 >/dev/null 2>&1; then
  printf '%s\n' 'expected step mismatch rejection' >&2
  exit 1
fi
if printf '%s\n' "${ENVELOPE/\"step\":0/\"step\":1}" | env \
  CTX_AGENT_LAUNCH=sdk-envelope-v1 CTX_RUN_ID=run-1 CTX_AGENT_STEP=1 \
  "$AGENT" --cortexfs-sdk-envelope-v1 >/dev/null 2>&1; then
  printf '%s\n' 'expected continuation rejection' >&2
  exit 1
fi

invoke_fixture() {
  env CTX_AGENT_LAUNCH=sdk-envelope-v1 CTX_RUN_ID=run-1 CTX_AGENT_STEP=0 \
    "$AGENT" --cortexfs-sdk-envelope-v1
}
expect_reject() {
  local label=$1 fixture=$2
  if invoke_fixture <"$fixture" >/dev/null 2>&1; then
    printf 'expected %s rejection\n' "$label" >&2
    exit 1
  fi
}

printf '%s\n' "$ENVELOPE" | jq -c 'del(.event,.origin)' >"$ROOT/optional.jsonl"
invoke_fixture <"$ROOT/optional.jsonl" >/dev/null
printf '%s\n' "$ENVELOPE" | jq -c \
  '.origin={transport:"channel",future_field:"runtime-compatible"}' \
  >"$ROOT/origin-forward.jsonl"
invoke_fixture <"$ROOT/origin-forward.jsonl" >/dev/null
jq -cn '{schema:"cortexfs.agent-invocation/v1",run:"run-1",step:0,input:"",
  event:null,origin:null,history_messages:"",tool_context:"",observation:null}' \
  >"$ROOT/output-base-input.jsonl"
invoke_fixture <"$ROOT/output-base-input.jsonl" >"$ROOT/output-base.jsonl"
output_base_bytes=$(wc -c <"$ROOT/output-base.jsonl")
output_padding=$((262144 - output_base_bytes))
head -c "$output_padding" /dev/zero | tr '\0' o >"$ROOT/output-padding"
jq -cn --rawfile input "$ROOT/output-padding" \
  '{schema:"cortexfs.agent-invocation/v1",run:"run-1",step:0,input:$input,
    event:null,origin:null,history_messages:"",tool_context:"",observation:null}' \
  >"$ROOT/output-boundary-input.jsonl"
invoke_fixture <"$ROOT/output-boundary-input.jsonl" >"$ROOT/output-boundary.jsonl"
[[ $(wc -c <"$ROOT/output-boundary.jsonl") -eq 262144 ]]
jq -e '.content[0].text | endswith("o")' "$ROOT/output-boundary.jsonl" >/dev/null

printf '%s\n' "$ENVELOPE" | jq -c '.unexpected=true' >"$ROOT/unknown.jsonl"
printf '%s\n' "$ENVELOPE" | jq -c 'del(.observation)' >"$ROOT/missing.jsonl"
printf '%s\n' "$ENVELOPE" | jq -c '.event=[]' >"$ROOT/event.jsonl"
printf '%s\n' "$ENVELOPE" | jq -c '.origin={transport:""}' >"$ROOT/origin.jsonl"
expect_reject unknown-field "$ROOT/unknown.jsonl"
expect_reject missing-observation "$ROOT/missing.jsonl"
expect_reject invalid-event "$ROOT/event.jsonl"
expect_reject invalid-origin "$ROOT/origin.jsonl"

head -c 65537 /dev/zero | tr '\0' h >"$ROOT/history"
jq -cn --rawfile history "$ROOT/history" \
  '{schema:"cortexfs.agent-invocation/v1",run:"run-1",step:0,input:"x",
    event:null,origin:null,history_messages:$history,tool_context:"",observation:null}' \
  >"$ROOT/context.jsonl"
expect_reject oversized-context "$ROOT/context.jsonl"

jq -cn '{schema:"cortexfs.agent-invocation/v1",run:"run-1",step:0,input:"",
  event:null,origin:null,history_messages:"",tool_context:"",observation:null}' \
  >"$ROOT/boundary-base.jsonl"
base_bytes=$(wc -c <"$ROOT/boundary-base.jsonl")
padding=$((1048576 - base_bytes))
head -c "$padding" /dev/zero | tr '\0' a >"$ROOT/padding"
jq -cn --rawfile input "$ROOT/padding" \
  '{schema:"cortexfs.agent-invocation/v1",run:"run-1",step:0,input:$input,
    event:null,origin:null,history_messages:"",tool_context:"",observation:null}' \
  >"$ROOT/boundary.jsonl"
[[ $(wc -c <"$ROOT/boundary.jsonl") -eq 1048576 ]]
invoke_fixture <"$ROOT/boundary.jsonl" >"$ROOT/boundary-output.jsonl"
[[ $(wc -c <"$ROOT/boundary-output.jsonl") -le 262144 ]]
jq -e '.type == "message"' "$ROOT/boundary-output.jsonl" >/dev/null
printf 'a' >>"$ROOT/padding"
jq -cn --rawfile input "$ROOT/padding" \
  '{schema:"cortexfs.agent-invocation/v1",run:"run-1",step:0,input:$input,
    event:null,origin:null,history_messages:"",tool_context:"",observation:null}' \
  >"$ROOT/oversized.jsonl"
expect_reject oversized-frame "$ROOT/oversized.jsonl"

printf '%s\n' 'shell agent smoke: ok'
