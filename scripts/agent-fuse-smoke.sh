#!/usr/bin/env bash
set -euo pipefail

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'skipping FUSE agent smoke: missing command: %s\n' "$1" >&2
        exit 0
    fi
}

for command in bwrap findmnt git python3 rustc systemctl systemd-run; do
    need "$command"
done

if ! command -v fusermount3 >/dev/null 2>&1 && ! command -v fusermount >/dev/null 2>&1; then
    printf 'skipping FUSE agent smoke: missing fusermount3/fusermount\n' >&2
    exit 0
fi
if [ ! -x /usr/bin/ctxterm ]; then
    printf 'skipping FUSE agent smoke: /usr/bin/ctxterm unavailable\n' >&2
    exit 0
fi
user_systemd_state=$(systemctl --user is-system-running 2>/dev/null || true)
case "$user_systemd_state" in
running | degraded) ;;
*)
    printf 'skipping FUSE agent smoke: user systemd unavailable (%s)\n' "$user_systemd_state" >&2
    exit 0
    ;;
esac

repo_root=$(pwd -P)
mountpoint="$repo_root/tests/mounts/cortexfs"
root=$(mktemp -d)
workspace=$(mktemp -d)
provider_port_file=$(mktemp)
mount_log=/tmp/cortexfs-agent-fuse-mount.log
start_log=/tmp/cortexfs-agent-fuse-start.log
mount_pid=
provider_pid=

unmount_fuse() {
    fusermount3 -u "$mountpoint" >/dev/null 2>&1 ||
        fusermount -u "$mountpoint" >/dev/null 2>&1 ||
        umount "$mountpoint" >/dev/null 2>&1 ||
        true
}

cleanup() {
    local status=$?
    CTX_ROOT="$mountpoint" "$repo_root/target/debug/ctx" agent stop coder >/dev/null 2>&1 || true
    unmount_fuse
    if [ -n "${mount_pid:-}" ]; then
        kill "$mount_pid" >/dev/null 2>&1 || true
    fi
    if [ -n "${provider_pid:-}" ]; then
        kill "$provider_pid" >/dev/null 2>&1 || true
    fi
    rm -f "$provider_port_file"
    if [ "$status" -eq 0 ] && [ "${CORTEXFS_KEEP_FUSE_SMOKE:-0}" != 1 ]; then
        rm -rf "$root" "$workspace"
    else
        printf 'preserving FUSE agent smoke artifacts for diagnosis:\n' >&2
        printf '  root: %s\n' "$root" >&2
        printf '  workspace: %s\n' "$workspace" >&2
        printf '  mount log: %s\n' "$mount_log" >&2
        printf '  start log: %s\n' "$start_log" >&2
    fi
    exit "$status"
}
trap cleanup EXIT

mkdir -p "$(dirname "$mountpoint")"
unmount_fuse
mkdir -p "$mountpoint"
if [ -n "$(find "$mountpoint" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
    printf 'FUSE agent smoke mountpoint is not empty: %s\n' "$mountpoint" >&2
    exit 1
fi

cargo build --locked -p cortexfs \
  --bin ctx \
  --bin tsh \
  --bin cortexfs-agent-runtime \
  --bin cortexfs-object-runner \
  --bin cortexfs-mount \
  --all-features >/dev/null

"$repo_root/target/debug/ctx" bootstrap "$root" >/dev/null
cp "$repo_root/target/debug/cortexfs-object-runner" "$root/bin/cortexfs-object-runner"
cp "$repo_root/target/debug/tsh" "$root/bin/tsh"
chmod 755 "$root/bin/cortexfs-object-runner" "$root/bin/tsh"

python3 - "$provider_port_file" <<'PY' &
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

port_file = sys.argv[1]

def tool_call(call_id, command):
    return json.dumps({
        "type": "tool_call",
        "run": "provider",
        "id": call_id,
        "name": "tsh",
        "arguments": {"args": command},
    }, separators=(",", ":"))

def provider_text(body):
    text = json.dumps(body, ensure_ascii=False)
    if "call-4" in text:
        return "fuse-mounted agent source edit complete"
    if "call-3" in text:
        return tool_call("call-4", [
            "shell.exec",
            "rustc --test /workspace/src/lib.rs -o /tmp/fuse_selfedit_test && /tmp/fuse_selfedit_test",
        ])
    if "call-2" in text:
        return tool_call("call-3", [
            "fs.write",
            "/workspace/src/lib.rs",
            "pub fn improved() -> bool {\n    true\n}\n\n#[test]\nfn programming_agent_improves_source() {\n    assert!(improved());\n}\n",
        ])
    if "call-1" in text:
        return tool_call("call-2", ["fs.read", "/workspace/src/lib.rs"])
    return tool_call("call-1", [
        "shell.exec",
        "test -f /workspace/AGENTS.md && cat /workspace/AGENTS.md; git -C /workspace status --short 2>/dev/null || true",
    ])

class Handler(BaseHTTPRequestHandler):
    def log_message(self, _format, *_args):
        return

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        body = json.loads(self.rfile.read(length) or b"{}")
        text = provider_text(body)
        if body.get("stream"):
            payload = json.dumps({"choices": [{"delta": {"content": text}}]}, separators=(",", ":"))
            data = f"data: {payload}\n\ndata: [DONE]\n\n".encode()
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        payload = json.dumps({
            "choices": [{"message": {"content": text}}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1},
        }, separators=(",", ":")).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
with open(port_file, "w", encoding="utf-8") as handle:
    handle.write(str(server.server_address[1]))
server.serve_forever()
PY
provider_pid=$!
for _ in $(seq 1 50); do
    if [ -s "$provider_port_file" ]; then
        break
    fi
    sleep 0.1
done
if [ ! -s "$provider_port_file" ]; then
    printf 'FUSE agent smoke provider failed to start\n' >&2
    exit 1
fi
provider_port=$(cat "$provider_port_file")
mkdir -p "$root/shared/providers.d"
cat >"$root/shared/providers.d/smoke.json" <<PROVIDER
{
  "name": "smoke",
  "base_url": "http://127.0.0.1:${provider_port}/v1",
  "formats": ["openai.chat"]
}
PROVIDER

model_name=smoke/fuse-selfedit
model_control="$root/model/smoke/fuse-selfedit.d"
mkdir -p "$root/model/smoke" "$model_control/hooks/pre.d" "$model_control/hooks/post.d"
printf '%s\n' "$model_name" >"$model_control/id"
printf 'openai-chat\n' >"$model_control/driver"
printf 'chat\n' >"$model_control/cap"
printf 'auto\n' >"$model_control/effort"
printf '\n' >"$model_control/default"
printf '\n' >"$model_control/fallback"
printf 'none\n' >"$model_control/session"
printf 'ready\n' >"$model_control/status"
printf '\n' >"$model_control/log"
: >"$root/model/smoke/fuse-selfedit"
chmod 755 "$root/model/smoke/fuse-selfedit"
printf '%s\n' "$model_name" >"$root/agent/coder.d/model"
printf 'CTX_ROOT=/ctx\nCTX_PROVIDER_CONFIG_DIR=/ctx/shared/providers.d\n' >"$root/agent/coder.d/env"
cat >"$root/agent/coder.d/policy" <<'POLICY'
allow coder_t model:smoke/fuse-selfedit use
allow coder_t tool:tsh execute
allow coder_t tool:fs.read execute
allow coder_t tool:fs.write execute
allow coder_t tool:shell.exec execute
allow coder_t network:default connect
POLICY
session_root="$root/home/$(id -u)/agent/coder/session"
session_dir="$session_root/fuse"
context_dir="$session_dir/context"
mkdir -p \
    "$context_dir/pinned" \
    "$context_dir/swap/chunk" \
    "$context_dir/dedup/blob" \
    "$context_dir/child" \
    "$session_root/index/by-cwd" \
    "$session_root/index/by-hash" \
    "$session_root/index/by-uuid"
for file in messages.jsonl events.jsonl latest.md; do
    : >"$session_dir/$file"
done
printf 'idle\n' >"$session_dir/state"
printf '/workspace\n' >"$session_dir/cwd"
printf '0\n' >"$session_dir/created_at"
printf '0\n' >"$session_dir/updated_at"
printf '{"client":"ctx","model":"smoke/fuse-selfedit","scope":"private"}\n' >"$session_dir/meta.json"
printf '0\n' >"$context_dir/budget"
printf '{"session":"fuse","items":[]}\n' >"$context_dir/pack.json"
for file in pack.md summary.md facts.jsonl decisions.jsonl todo.md refs.jsonl; do
    : >"$context_dir/$file"
done
: >"$context_dir/swap/index.jsonl"
: >"$context_dir/dedup/index.jsonl"
: >"$session_root/index/list"
printf 'fuse\n' >"$session_root/index/current"

mkdir -p "$workspace/src"
cat >"$workspace/AGENTS.md" <<'AGENTS'
Read before editing.
Preserve unrelated user changes.
Run verification before reporting success.
AGENTS
cat >"$workspace/src/lib.rs" <<'RS'
pub fn improved() -> bool {
    false
}

#[test]
fn programming_agent_improves_source() {
    assert!(improved());
}
RS
printf 'baseline project note\n' >"$workspace/README.md"
git -C "$workspace" init -q
git -C "$workspace" config user.email cortexfs-smoke@example.invalid
git -C "$workspace" config user.name "CortexFS Smoke"
git -C "$workspace" add AGENTS.md README.md src/lib.rs
git -C "$workspace" commit -qm baseline
printf 'baseline project note\nuser dirty change\n' >"$workspace/README.md"

"$repo_root/target/debug/cortexfs-mount" --source "$root" "$mountpoint" >"$mount_log" 2>&1 &
mount_pid=$!
for _ in $(seq 1 50); do
    if findmnt "$mountpoint" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if ! findmnt "$mountpoint" >/dev/null 2>&1; then
    printf 'FUSE agent smoke mount failed:\n' >&2
    cat "$mount_log" >&2
    exit 1
fi

CTX_ROOT="$mountpoint" "$repo_root/target/debug/ctx" doctor >/dev/null
CTX_ROOT="$mountpoint" "$repo_root/target/debug/ctx" agent start coder \
    --session fuse \
    --cwd /workspace \
    --no-default-workspace \
    --mount "$workspace" /workspace rw >"$start_log" 2>&1 || {
    printf 'FUSE agent smoke start failed:\n' >&2
    cat "$start_log" >&2
    exit 1
}

output=$(cd "$workspace" && CTX_ROOT="$mountpoint" "$repo_root/target/debug/ctx" agent send coder \
    --session fuse \
    --raw \
    'improve the mounted CortexFS source fixture and verify it')
CTX_ROOT="$mountpoint" "$repo_root/target/debug/ctx" agent stop coder >/dev/null 2>&1 || true

case "$output" in
*"fuse-mounted agent source edit complete"*'"status":"ok"'*) ;;
*)
    printf 'unexpected FUSE agent smoke output:\n%s\n' "$output" >&2
    exit 1
    ;;
esac
grep -q 'true' "$workspace/src/lib.rs"
rustc --test "$workspace/src/lib.rs" -o "$workspace/fuse-selfedit-test"
"$workspace/fuse-selfedit-test" >/dev/null
grep -q 'user dirty change' "$workspace/README.md"
uid=$(id -u)
grep -q 'fuse-mounted agent source edit complete' "$root/home/$uid/agent/coder/session/fuse/messages.jsonl"
grep -q '^done$' "$root/home/$uid/agent/coder/session/fuse/state"
printf 'agent FUSE smoke passed\n'
