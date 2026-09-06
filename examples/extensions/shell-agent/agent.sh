#!/bin/sh
set -eu

[ "$#" -eq 1 ] && [ "$1" = "--cortexfs-sdk-envelope-v1" ] || exit 2
[ "${CTX_AGENT_LAUNCH:-}" = "sdk-envelope-v1" ] || exit 2
[ -n "${CTX_RUN_ID:-}" ] && [ -n "${CTX_AGENT_STEP:-}" ] || exit 2
command -v jq >/dev/null 2>&1 || {
  printf '%s\n' 'shell agent requires jq' >&2
  exit 2
}

IFS= read -r envelope || exit 2
if IFS= read -r extra || [ -n "${extra:-}" ]; then
  exit 2
fi
bytes=$(LC_ALL=C printf '%s' "$envelope" | wc -c)
# The canonical 1 MiB limit includes the required trailing newline.
[ "$bytes" -le 1048575 ] || exit 2
prefix=${SHELL_AGENT_PREFIX:-FieldNotes}
session=${CTX_SESSION:-default}
[ "$(LC_ALL=C printf '%s' "$prefix" | wc -c)" -le 1024 ] || exit 2
[ "$(LC_ALL=C printf '%s' "$session" | wc -c)" -le 1024 ] || exit 2
result=$(printf '%s' "$envelope" | jq -ce \
  --arg run "$CTX_RUN_ID" \
  --argjson step "$CTX_AGENT_STEP" \
  --arg prefix "$prefix" \
  --arg session "$session" \
  'def no_ascii_control:
     type == "string" and (test("[\u0000-\u001f\u007f]") | not);
   def bounded_json($limit): (tojson | utf8bytelength) <= $limit;
   def optional_clean($name):
     (has($name) | not) or (.[$name] == null)
       or ((.[$name] | type) == "string"
           and (.[$name] | length) > 0
           and (.[$name] | no_ascii_control));
   def response($text):
     {type:"message",run:$run,role:"assistant",content:[{type:"text",text:$text}]};
   def valid_origin:
     . == null or (
       type == "object"
       and has("transport")
       and (.transport | length) > 0
       and (.transport | no_ascii_control)
       and optional_clean("endpoint")
       and optional_clean("identity")
       and optional_clean("conversation")
       and optional_clean("thread")
       and ((has("metadata") | not) or (
         (.metadata | type) == "object"
         and all(.metadata | to_entries[];
           (.key | length) > 0
           and (.key | no_ascii_control)
           and (.value | no_ascii_control))))
       and bounded_json(65536));
   . as $envelope |
   if type == "object"
     and ((keys - ["event","history_messages","input","observation","origin",
                   "run","schema","step","tool_context"]) | length) == 0
     and all(["history_messages","input","observation","run","schema","step",
              "tool_context"][]; . as $key | $envelope | has($key))
     and .schema == "cortexfs.agent-invocation/v1"
     and .run == $run
     and .step == $step
     and .step == 0
     and (.input | type == "string")
     and (.history_messages | type == "string")
     and (.history_messages | utf8bytelength) <= 65536
     and (.tool_context | type == "string")
     and (.tool_context | utf8bytelength) <= 65536
     and ((.event? // null) == null or ((.event | type) == "object"
                                         and (.event | bounded_json(65536))))
     and ((.origin? // null) | valid_origin)
     and .observation == null
   then (response($prefix + "[" + $session + "]: " + .input) as $full |
     if (($full | tojson | utf8bytelength) + 1) <= 262144
     then $full
     else response($prefix + "[" + $session + "]: received "
                   + ((.input | utf8bytelength) | tostring) + " input bytes")
     end)
   else error("invalid CortexFS agent envelope")
   end') || exit 2
[ "$(LC_ALL=C printf '%s' "$result" | wc -c)" -le 262143 ] || exit 2
printf '%s\n' "$result"
