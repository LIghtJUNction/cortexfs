#!/usr/bin/env bash
set -euo pipefail

model=${CORTEXFS_LIVE_MODEL:-smollm2:135m}
ollama_url=${OLLAMA_HOST:-http://127.0.0.1:11434}

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing required command: %s\n' "$1" >&2
        exit 127
    fi
}

json_payload() {
    node - "$model" "$1" <<'NODE'
const [model, prompt] = process.argv.slice(2);
process.stdout.write(JSON.stringify({
  model,
  prompt,
  stream: false,
  options: { temperature: 0 }
}));
NODE
}

json_response_text() {
    node -e '
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => {
  const value = JSON.parse(input);
  process.stdout.write(String(value.response ?? ""));
});
'
}

ollama_generate() {
    local prompt=$1
    local payload
    payload=$(json_payload "$prompt")
    curl -fsS "$ollama_url/api/generate" \
        -H 'Content-Type: application/json' \
        -d "$payload" |
        json_response_text
}

need curl
need git
need node
need rustc

tags=$(curl -fsS "$ollama_url/api/tags") || {
    printf 'ollama is not reachable at %s\n' "$ollama_url" >&2
    exit 2
}
if ! printf '%s' "$tags" | node -e '
const model = process.argv[1];
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => {
  const tags = JSON.parse(input);
  const ok = (tags.models || []).some(entry => entry.name === model || entry.model === model);
  process.exit(ok ? 0 : 1);
});
 ' "$model"
then
    printf 'ollama model is not installed: %s\n' "$model" >&2
    printf 'install it explicitly, for example: ollama pull %s\n' "$model" >&2
    exit 2
fi

exact=$(ollama_generate 'Reply with exactly: cortexfs-ok')
if [ "$(printf '%s' "$exact" | tr -d '\r\n' | sed 's/^ *//;s/ *$//')" != "cortexfs-ok" ]; then
    printf 'live model %s failed exact-response gate.\n' "$model" >&2
    printf 'expected: cortexfs-ok\nactual: %s\n' "$exact" >&2
    printf 'not running the source-edit agent gate with this model.\n' >&2
    exit 2
fi

tool_prompt='Output exactly one JSON object line and no prose:
{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["tools"]}}'
tool_call=$(ollama_generate "$tool_prompt")
if ! printf '%s' "$tool_call" | node -e '
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => {
  const text = input.trim();
  let value;
  try {
    value = JSON.parse(text);
  } catch (_error) {
    process.exit(1);
  }
  const ok = value.type === "tool_call"
    && value.id === "call-1"
    && value.name === "tsh"
    && Array.isArray(value.arguments?.args)
    && value.arguments.args.length === 1
    && value.arguments.args[0] === "tools";
  process.exit(ok ? 0 : 1);
});
'
then
    printf 'live model %s failed tool-call protocol gate.\n' "$model" >&2
    printf 'actual response:\n%s\n' "$tool_call" >&2
    exit 2
fi

printf 'live model protocol smoke passed: %s\n' "$model"

for command in bwrap systemctl systemd-run; do
    need "$command"
done
if [ ! -x /usr/bin/ctxterm ]; then
    printf 'missing required command: /usr/bin/ctxterm\n' >&2
    exit 127
fi
user_systemd_state=$(systemctl --user is-system-running 2>/dev/null || true)
case "$user_systemd_state" in
running | degraded) ;;
*)
    printf 'user systemd is not available (%s); cannot start live agent socket\n' "$user_systemd_state" >&2
    exit 2
    ;;
esac

repo_root=$(pwd -P)
root=$(mktemp -d)
workspace=$(mktemp -d)
source_file="$workspace/crates/cortexfs/src/self_improve_smoke.rs"
source_test_bin="$workspace/self_improve_smoke_test"
user_dirty_file="$workspace/README.md"
send_log=/tmp/cortexfs-agent-live-send.log
smoke_failed=0

cleanup() {
    local status=$?
    "$repo_root/target/debug/ctx" --root "$root" agent stop coder >/dev/null 2>&1 || true
    if [ "$status" -eq 0 ] && [ "$smoke_failed" -eq 0 ] && [ "${CORTEXFS_KEEP_LIVE_SMOKE:-0}" != 1 ]; then
        rm -rf "$root" "$workspace"
        return
    fi
    printf 'preserving live smoke artifacts for diagnosis:\n' >&2
    printf '  root: %s\n' "$root" >&2
    printf '  workspace: %s\n' "$workspace" >&2
    printf '  model wrapper: %s\n' "$root/model/debug/ollama-live" >&2
    printf '  start log: %s\n' /tmp/cortexfs-agent-live-start.log >&2
    printf '  send log: %s\n' "$send_log" >&2
}
trap cleanup EXIT

write_source_fixture() {
    mkdir -p "$(dirname "$source_file")"
    cat >"$workspace/AGENTS.md" <<'EOF'
Read the current source before editing.
Do not overwrite unrelated user changes.
Run verification before reporting success.
EOF
    cat >"$source_file" <<'EOF'
pub fn agent_quality_label() -> &'static str {
    "todo"
}

#[test]
fn programming_agent_improves_source() {
    assert_eq!(agent_quality_label(), "cortexfs-agent-source");
}
EOF
    printf 'baseline project note\n' >"$user_dirty_file"
    git -C "$workspace" init -q
    git -C "$workspace" config user.email cortexfs-smoke@example.invalid
    git -C "$workspace" config user.name "CortexFS Smoke"
    git -C "$workspace" add AGENTS.md README.md crates/cortexfs/src/self_improve_smoke.rs
    git -C "$workspace" commit -qm baseline
    printf 'baseline project note\nuser dirty change\n' >"$user_dirty_file"
}

assert_source_improved() {
    grep -q '"cortexfs-agent-source"' "$source_file"
    rustc --test "$source_file" -o "$source_test_bin"
    "$source_test_bin" >/dev/null
    grep -q 'user dirty change' "$user_dirty_file"
    status=$(git -C "$workspace" status --short)
    if [[ "$status" != *" M README.md"* ||
        "$status" != *" M crates/cortexfs/src/self_improve_smoke.rs"* ]]; then
        printf 'unexpected live workspace git status after source improvement:\n%s\n' "$status" >&2
        exit 1
    fi
}

assert_session_recorded() {
    local expected=$1
    local uid
    local latest
    local history
    uid=$(id -u)
    latest=$("$repo_root/target/debug/ctx" --root "$root" agent output coder --session live)
    history=$("$repo_root/target/debug/ctx" --root "$root" agent history coder --session live)
    grep -q "$expected" <<<"$latest"
    grep -q "$expected" <<<"$history"
    grep -q '^done$' "$root/home/$uid/agent/coder/session/live/state"
}

cargo build --locked -p cortexfs \
    --bin ctx \
    --bin tsh \
    --bin cortexfs-agent-runtime \
    --bin cortexfs-object-runner \
    --all-features >/dev/null
cargo run --locked -p cortexfs --bin ctx --all-features -- bootstrap "$root" >/dev/null
cp "$repo_root/target/debug/cortexfs-object-runner" "$root/bin/cortexfs-object-runner"
cp "$repo_root/target/debug/tsh" "$root/bin/tsh"
chmod 755 "$root/bin/cortexfs-object-runner" "$root/bin/tsh"
grep -Fq "exec '/ctx/bin/cortexfs-object-runner' \"\$0\" \"\$@\"" "$root/agent/coder"
for tool in fs.read fs.write shell.exec tsh.config; do
    grep -Fq "exec '/ctx/bin/cortexfs-object-runner' \"\$0\" \"\$@\"" "$root/tool/$tool"
done
grep -Fq 'exec /ctx/bin/tsh "$@"' "$root/tool/tsh"
write_source_fixture

mkdir -p "$root/model/debug" "$root/model/debug/ollama-live.d"
cat >"$root/model/debug/ollama-live" <<EOF
#!/bin/sh
model=\${CORTEXFS_LIVE_MODEL:-$model}
ollama_url=\${OLLAMA_HOST:-$ollama_url}
user_input="\$*"
prompt=\$(cat <<'PROMPT'
You are a CortexFS coding agent running in /workspace.
Output exactly one JSON object line and no prose when a tool call is needed.
The first byte of your response must be { and the last byte must be }.
Never use Markdown fences. Never output tsh followed by a bracketed argument list. Never output top-level keys named tool or args.
Your only callable tool is tsh.
Use this exact sequence for this task:
1. If there is no Tool result call-1, output exactly:
{"type":"tool_call","id":"call-1","name":"tsh","arguments":{"args":["shell.exec","test -f /workspace/AGENTS.md && cat /workspace/AGENTS.md; git -C /workspace status --short 2>/dev/null || true"]}}
2. If there is Tool result call-1 but no Tool result call-2, output exactly:
{"type":"tool_call","id":"call-2","name":"tsh","arguments":{"args":["fs.read","/workspace/crates/cortexfs/src/self_improve_smoke.rs"]}}
3. If there is Tool result call-2 but no Tool result call-3, output exactly:
{"type":"tool_call","id":"call-3","name":"tsh","arguments":{"args":["fs.write","/workspace/crates/cortexfs/src/self_improve_smoke.rs","pub fn agent_quality_label() -> &'static str {\n    \"cortexfs-agent-source\"\n}\n\n#[test]\nfn programming_agent_improves_source() {\n    assert_eq!(agent_quality_label(), \"cortexfs-agent-source\");\n}\n"]}}
4. If there is Tool result call-3 but no Tool result call-4, output exactly:
{"type":"tool_call","id":"call-4","name":"tsh","arguments":{"args":["shell.exec","rustc --test /workspace/crates/cortexfs/src/self_improve_smoke.rs -o /tmp/self_improve_smoke && /tmp/self_improve_smoke"]}}
5. If there is Tool result call-4, reply exactly: source self-improvement live complete

Preserve unrelated user changes. Do not modify /workspace/README.md.
PROMPT
)
prompt=\$(printf '%s\n\nUser task:\n%s\n\nTool context:\n%s\n' "\$prompt" "\$user_input" "\${CTX_AGENT_TOOL_CONTEXT:-}")
payload=\$(MODEL="\$model" PROMPT="\$prompt" node -e 'process.stdout.write(JSON.stringify({model: process.env.MODEL, prompt: process.env.PROMPT, stream: false, options: {temperature: 0}}))')
curl -fsS "\$ollama_url/api/generate" -H 'Content-Type: application/json' -d "\$payload" |
node -e 'let input=""; process.stdin.setEncoding("utf8"); process.stdin.on("data", chunk => input += chunk); process.stdin.on("end", () => { const value = JSON.parse(input); process.stdout.write(String(value.response || "")); if (!String(value.response || "").endsWith("\n")) process.stdout.write("\n"); });'
EOF
chmod 755 "$root/model/debug/ollama-live"

printf 'debug/ollama-live\n' >"$root/agent/coder.d/model"
cat >"$root/agent/coder.d/policy" <<'EOF'
allow coder_t model:debug/ollama-live use
allow coder_t tool:tsh execute
allow coder_t tool:fs.read execute
allow coder_t tool:fs.write execute
allow coder_t tool:shell.exec execute
allow coder_t network:default connect
EOF
cat >"$root/agent/coder.d/mount" <<EOF
$root	/ctx	rw	rbind,nosuid,nodev
$workspace	/workspace	rw	rbind,nosuid,nodev
EOF

"$repo_root/target/debug/ctx" \
    --root "$root" \
    agent start coder \
    --session live \
    --cwd /workspace \
    --no-default-workspace >/tmp/cortexfs-agent-live-start.log 2>&1 || {
    printf 'ctx agent live start failed:\n' >&2
    cat /tmp/cortexfs-agent-live-start.log >&2
    exit 1
}

if ! output=$(
    "$repo_root/target/debug/ctx" \
        --root "$root" \
        agent send coder \
        --session live \
        --raw \
        'improve the CortexFS source fixture and verify it' 2>"$send_log"
); then
    smoke_failed=1
    printf 'ctx agent live send failed.\n' >&2
    printf 'stdout:\n%s\n' "$output" >&2
    printf 'stderr:\n' >&2
    cat "$send_log" >&2
    exit 1
fi
"$repo_root/target/debug/ctx" --root "$root" agent stop coder >/dev/null 2>&1 || true
case "$output" in
*"source self-improvement live complete"*'"status":"ok"'*) ;;
*)
    smoke_failed=1
    printf 'unexpected live source-edit agent output:\n%s\n' "$output" >&2
    if [ -s "$send_log" ]; then
        printf 'send stderr:\n' >&2
        cat "$send_log" >&2
    fi
    exit 1
    ;;
esac
assert_source_improved
assert_session_recorded 'source self-improvement live complete'

printf 'live model source-edit agent smoke passed: %s\n' "$model"
