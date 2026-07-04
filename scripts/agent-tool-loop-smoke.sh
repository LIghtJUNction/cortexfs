#!/usr/bin/env bash
set -euo pipefail

cargo test --locked -p cortexfs agent_tool_loop --all-features
cargo test --locked -p cortexfs agent_tool_call_executes_visible_tsh_for_search_and_load --all-features
cargo test --locked -p cortexfs --bin tsh tool_context --all-features
cargo build --locked -p cortexfs \
    --bin ctx \
    --bin tsh \
    --bin cortexfs-agent-runtime \
    --bin cortexfs-object-runner \
    --all-features

tools=$(cargo run -p cortexfs --bin tsh -- tools)
case "$tools" in
  *fs.read*shell.exec*tsh*) ;;
  *)
    printf 'unexpected tsh tools output:\n%s\n' "$tools" >&2
    exit 1
    ;;
esac

repl=$(printf 'load fs.read\nloads\nunload fs.read\nloads\nexit\n' |
  cargo run -p cortexfs --bin tsh --)
case "$repl" in
  *"loaded fs.read"*"/ctx/tool/fs.read"*metadata*"unloaded fs.read"*) ;;
  *)
    printf 'unexpected tsh repl output:\n%s\n' "$repl" >&2
    exit 1
    ;;
esac

if command -v bwrap >/dev/null 2>&1; then
    repo_root=$(pwd -P)
    root=$(mktemp -d)
    workspace=$(mktemp -d)
    cleanup() {
        "$repo_root/target/debug/ctx" --root "$root" agent stop coder >/dev/null 2>&1 || true
        rm -rf "$root" "$workspace"
    }
    trap cleanup EXIT
    source_file="$workspace/crates/cortexfs/tests/self_bootstrap_smoke.rs"
    source_test_bin="$workspace/self_bootstrap_smoke_test"
    agent_verify_command="test -f /workspace/Cargo.toml && test -f /workspace/crates/cortexfs/Cargo.toml && rustc --test /workspace/crates/cortexfs/tests/self_bootstrap_smoke.rs -o /tmp/self_bootstrap_smoke && /tmp/self_bootstrap_smoke"
    user_dirty_file="$workspace/README.md"
    write_self_bootstrap_workspace() {
        find "$workspace" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
        git -C "$repo_root" archive --format=tar HEAD | tar -C "$workspace" -xf -
        rm -f "$source_test_bin"
        test -f "$workspace/AGENTS.md"
        test ! -e "$source_file"
        if command -v git >/dev/null 2>&1; then
            git -C "$workspace" init -q
            git -C "$workspace" config user.email cortexfs-smoke@example.invalid
            git -C "$workspace" config user.name "CortexFS Smoke"
            git -C "$workspace" add .
            git -C "$workspace" commit -qm baseline
        fi
        printf '\nself-bootstrap dirty note\n' >>"$user_dirty_file"
    }
    assert_source_improved() {
        grep -q 'self_bootstrap_agent_updates_cortexfs_source' "$source_file"
        test -f "$workspace/Cargo.toml"
        test -f "$workspace/crates/cortexfs/Cargo.toml"
        rustc --test "$source_file" -o "$source_test_bin"
        "$source_test_bin" >/dev/null
        grep -q 'self-bootstrap dirty note' "$user_dirty_file"
        if [ -d "$workspace/.git" ]; then
            status=$(git -C "$workspace" status --short)
            if [[ "$status" != *" M README.md"* ||
                "$status" != *"?? crates/cortexfs/tests/self_bootstrap_smoke.rs"* ]]; then
                printf 'unexpected workspace git status after source improvement:\n%s\n' "$status" >&2
                exit 1
            fi
        fi
    }
    assert_session_recorded() {
        local session=$1
        local expected=$2
        local uid
        local latest
        local history
        uid=$(id -u)
        latest=$("$repo_root/target/debug/ctx" --root "$root" agent output coder --session "$session")
        history=$("$repo_root/target/debug/ctx" --root "$root" agent history coder --session "$session")
        grep -q "$expected" <<<"$latest"
        grep -q "$expected" <<<"$history"
        grep -q '^done$' "$root/home/$uid/agent/coder/session/$session/state"
    }
    assert_codex_style_final_output() {
        local output=$1
        grep -q 'changed: crates/cortexfs/tests/self_bootstrap_smoke.rs' <<<"$output"
        grep -q "verified: $agent_verify_command" <<<"$output"
        grep -q 'diff: git -C /workspace diff --stat && git -C /workspace diff -- crates/cortexfs/tests/self_bootstrap_smoke.rs' <<<"$output"
    }

    host_runner="$repo_root/target/debug/cortexfs-object-runner"
    host_tsh="$repo_root/target/debug/tsh"
    runner="/ctx/bin/cortexfs-object-runner"
    cargo run --locked -p cortexfs --bin ctx --all-features -- bootstrap "$root" >/dev/null
    cp "$host_runner" "$root/bin/cortexfs-object-runner"
    cp "$host_tsh" "$root/bin/tsh"
    chmod 755 "$root/bin/cortexfs-object-runner" "$root/bin/tsh"
    grep -Fq "exec '/ctx/bin/cortexfs-object-runner' \"\$0\" \"\$@\"" "$root/agent/coder"
    for tool in fs.read fs.write fs.replace shell.exec tsh.config; do
        grep -Fq "exec '/ctx/bin/cortexfs-object-runner' \"\$0\" \"\$@\"" "$root/tool/$tool"
    done
    grep -Fq 'exec /ctx/bin/tsh "$@"' "$root/tool/tsh"
    grep -Fq 'writable project checkout mounted at `/workspace`' "$root/agent/coder.d/system.md"
    grep -Fq 'fs.replace' "$root/agent/coder.d/system.md"
    grep -Fq 'do not stop at a plan' "$root/agent/coder.d/system.md"
    grep -Fq 'implement the requested change directly through `tsh`' "$root/agent/coder.d/system.md"
    grep -Fq 'formatter, static check, lint, and focused tests' "$root/agent/coder.d/system.md"
    grep -Fq 'git status --short' "$root/agent/coder.d/system.md"
    grep -Fq 'nearest applicable `AGENTS.md`' "$root/agent/coder.d/system.md"
    grep -Fq 'find /workspace -name AGENTS.md -print' "$root/agent/coder.d/system.md"
    grep -Fq 'real build, test, and git evidence' "$root/agent/coder.d/system.md"
    grep -Fq 'Never run destructive git commands' "$root/agent/coder.d/system.md"
    write_self_bootstrap_workspace

    mkdir -p "$root/model/debug" "$root/model/debug/selfedit.d"
cat >"$root/model/debug/selfedit" <<'EOF'
#!/bin/sh
case "${CTX_AGENT_TOOL_CONTEXT:-}" in
    *"call-4"*)
    printf '{"type":"delta","run":"%s","text":"source self-improvement smoke complete\nchanged: crates/cortexfs/tests/self_bootstrap_smoke.rs\nverified: test -f /workspace/Cargo.toml && test -f /workspace/crates/cortexfs/Cargo.toml && rustc --test /workspace/crates/cortexfs/tests/self_bootstrap_smoke.rs -o /tmp/self_bootstrap_smoke && /tmp/self_bootstrap_smoke\ndiff: git -C /workspace diff --stat && git -C /workspace diff -- crates/cortexfs/tests/self_bootstrap_smoke.rs"}\n' "$CTX_RUN_ID"
    printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
    ;;
  *"call-3"*)
    printf '{"type":"tool_call","run":"%s","id":"call-4","name":"tsh","arguments":{"args":["shell.exec","git -C /workspace diff --stat && git -C /workspace diff -- crates/cortexfs/tests/self_bootstrap_smoke.rs"]}}\n' "$CTX_RUN_ID"
    ;;
  *"call-2"*)
    printf '{"type":"tool_call","run":"%s","id":"call-3","name":"tsh","arguments":{"args":["shell.exec","test -f /workspace/Cargo.toml && test -f /workspace/crates/cortexfs/Cargo.toml && rustc --test /workspace/crates/cortexfs/tests/self_bootstrap_smoke.rs -o /tmp/self_bootstrap_smoke && /tmp/self_bootstrap_smoke"]}}\n' "$CTX_RUN_ID"
    ;;
  *"call-1"*"docs/DESIGN.md"*)
    printf '{"type":"tool_call","run":"%s","id":"call-2","name":"tsh","arguments":{"args":["fs.write","/workspace/crates/cortexfs/tests/self_bootstrap_smoke.rs","#[test]\\nfn self_bootstrap_agent_updates_cortexfs_source() {\\n    assert_eq!(\\"cortexfs-self-bootstrap\\", \\"cortexfs-self-bootstrap\\");\\n}\\n"]}}\n' "$CTX_RUN_ID"
    ;;
  *"call-1"*)
    printf '{"type":"error","run":"%s","code":"EIO","message":"missing nearest source AGENTS.md evidence"}\n' "$CTX_RUN_ID"
    ;;
  *"Current request context:"*"Host workspace mounted at \`/workspace\`"*)
    printf '{"type":"tool_call","run":"%s","id":"call-1","name":"tsh","arguments":{"args":["shell.exec","find /workspace -name AGENTS.md -print; cat /workspace/AGENTS.md /workspace/crates/cortexfs/src/AGENTS.md; git -C /workspace status --short"]}}\n' "$CTX_RUN_ID"
    ;;
  "")
    printf '{"type":"tool_call","run":"%s","id":"call-1","name":"tsh","arguments":{"args":["shell.exec","find /workspace -name AGENTS.md -print; cat /workspace/AGENTS.md /workspace/crates/cortexfs/src/AGENTS.md; git -C /workspace status --short"]}}\n' "$CTX_RUN_ID"
    ;;
  *)
    printf '{"type":"error","run":"%s","code":"EIO","message":"unexpected context"}\n' "$CTX_RUN_ID"
    exit 2
    ;;
esac
EOF
    chmod 755 "$root/model/debug/selfedit"

    printf 'debug/selfedit\n' >"$root/agent/coder.d/model"
    cat >"$root/agent/coder.d/policy" <<'EOF'
allow coder_t model:debug/selfedit use
allow coder_t tool:tsh execute
allow coder_t tool:fs.read execute
allow coder_t tool:fs.write execute
allow coder_t tool:fs.replace execute
allow coder_t tool:shell.exec execute
allow coder_t network:default connect
EOF
    cat >"$root/agent/coder.d/mount" <<EOF
$root	/ctx	rw	rbind,nosuid,nodev
$workspace	/workspace	rw	rbind,nosuid,nodev
EOF

    parent_args=()
    parent=$(dirname "$repo_root")
    while [ "$parent" != "/" ]; do
        parent_args=(--dir "$parent" "${parent_args[@]}")
        parent=$(dirname "$parent")
    done

    output=$(
        bwrap \
            --clearenv \
            --die-with-parent \
            --unshare-pid \
            --unshare-net \
            --proc /proc \
            --dev /dev \
            --tmpfs /tmp \
            --dir /run \
            "${parent_args[@]}" \
            --ro-bind /usr /usr \
            --ro-bind /etc /etc \
            --ro-bind "$repo_root" "$repo_root" \
            --symlink usr/bin /bin \
            --symlink usr/lib /lib \
            --symlink usr/lib /lib64 \
            --bind "$root" /ctx \
            --bind "$workspace" /workspace \
            --chdir /workspace \
            --setenv PATH /usr/bin:/bin \
            --setenv CTX_ROOT /ctx \
            --setenv CTX_SOURCE /ctx \
            --setenv CTX_RUN_ID smoke \
            --setenv CTX_SESSION smoke \
            --setenv CTX_AGENT_HISTORY_MESSAGES '' \
    "$runner" /ctx/agent/coder 'improve the CortexFS source tree itself and verify it'
    )
    case "$output" in
    *"source self-improvement smoke complete"*'"status":"ok"'*) ;;
    *)
        printf 'unexpected source self-improvement smoke output:\n%s\n' "$output" >&2
        exit 1
        ;;
    esac
    assert_codex_style_final_output "$output"
    assert_source_improved
    printf 'source self-improvement smoke passed\n' >&2

    if command -v systemd-run >/dev/null 2>&1 &&
        systemctl --user is-system-running >/dev/null 2>&1 &&
        [ -x /usr/bin/ctxterm ]; then
        write_self_bootstrap_workspace
        "$repo_root/target/debug/ctx" \
            --root "$root" \
            agent start coder \
            --session smoke \
            --cwd /workspace \
            --no-default-workspace >/tmp/cortexfs-agent-start-smoke.log 2>&1 || {
            printf 'ctx agent start source self-improvement smoke failed:\n' >&2
            cat /tmp/cortexfs-agent-start-smoke.log >&2
            exit 1
        }
        output=$(
            "$repo_root/target/debug/ctx" \
                --root "$root" \
                agent send coder \
                --session smoke \
                --raw \
                'improve the CortexFS source tree itself and verify it'
        )
        "$repo_root/target/debug/ctx" --root "$root" agent stop coder >/dev/null 2>&1 || true
        case "$output" in
        *"source self-improvement smoke complete"*'"status":"ok"'*) ;;
        *)
            printf 'unexpected ctx agent start/send source self-improvement smoke output:\n%s\n' "$output" >&2
            exit 1
            ;;
        esac
        assert_codex_style_final_output "$output"
        assert_source_improved
        assert_session_recorded smoke 'source self-improvement smoke complete'
        assert_session_recorded smoke 'changed: crates/cortexfs/tests/self_bootstrap_smoke.rs'
        printf 'ctx agent start/send source self-improvement smoke passed\n' >&2
    else
        printf 'skipping ctx agent start/send source self-improvement smoke: user systemd or /usr/bin/ctxterm unavailable\n' >&2
    fi
else
    printf 'skipping bwrap source self-improvement smoke: bwrap not found\n' >&2
fi

printf 'agent tool-loop smoke passed\n'
