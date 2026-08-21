#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
. "$repo_root/scripts/resources.sh"
cd "$repo_root"

hosted_test=tests::installed_sdks_run_two_declared_native_tool_calls
scripts/test.sh cargo test --locked -p cortexfs-sdk-extension-fixture --test installed \
    "$hosted_test" -- --list | grep -Fqx "$hosted_test: test"
scripts/test.sh cargo test --locked -p cortexfs-sdk-extension-fixture --test installed \
    "$hosted_test" -- --exact
scripts/test.sh cargo test --locked -p cortexfs agent_tool_call_executes_visible_tsh_for_search_and_load --all-features
scripts/test.sh cargo test --locked -p cortexfs --bin tsh tool_context --all-features
cargo build --locked -p cortexfs \
    --bin ctx \
    --bin tsh \
    --bin cortexfs-agent-runtime \
    --bin cortexfs-object-runner \
    --all-features

tools=$(cargo run -p cortexfs --bin tsh -- tools -l)
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
    approval_root=
    approval_unit=
    approval_socket=
    systemd_run_system=()
    system_privilege=()
    systemctl_system=()
    systemd_fixture_ready=false
    systemd_skip_reason=
    if ! command -v systemctl >/dev/null 2>&1; then
        systemd_skip_reason='systemctl unavailable'
    elif ! command -v systemd-run >/dev/null 2>&1; then
        systemd_skip_reason='systemd-run unavailable'
    elif ! command -v pgrep >/dev/null 2>&1; then
        systemd_skip_reason='pgrep unavailable for runtime cleanup verification'
    else
        systemd_system_state=$(systemctl is-system-running 2>/dev/null || true)
        case "$systemd_system_state" in
          running | degraded)
            if [ "$(id -u)" -eq 0 ]; then
                systemd_run_system=(systemd-run --system --no-ask-password)
                systemctl_system=(systemctl --system --no-ask-password)
                systemd_fixture_ready=true
            elif command -v sudo >/dev/null 2>&1 &&
              sudo -n true >/dev/null 2>&1; then
                system_privilege=(sudo -n)
                systemd_run_system=(sudo -n systemd-run --system --no-ask-password)
                systemctl_system=(sudo -n systemctl --system --no-ask-password)
                systemd_fixture_ready=true
            else
                systemd_skip_reason='noninteractive system privilege unavailable'
            fi
            ;;
          *)
            systemd_skip_reason="system manager state is ${systemd_system_state:-unknown}"
            ;;
        esac
    fi
    cleanup_transient_unit() {
        local unit=$1
        if [ -n "$unit" ] && [ "${#systemctl_system[@]}" -gt 0 ]; then
            "${systemctl_system[@]}" stop \
                "$unit.socket" "$unit.service" >/dev/null 2>&1 || true
            "${systemctl_system[@]}" reset-failed \
                "$unit.socket" "$unit.service" >/dev/null 2>&1 || true
        fi
    }
    cleanup() {
        cleanup_transient_unit "$approval_unit"
        "$repo_root/target/debug/ctx" --root "$root" agent stop coder >/dev/null 2>&1 || true
        if [ -n "$approval_socket" ]; then
            rm -f "$approval_socket"
        fi
        if [ -n "$approval_root" ]; then
            "${system_privilege[@]}" rm -rf "$approval_root"
        fi
        rm -rf "$root" "$workspace"
    }
    trap cleanup EXIT
    snapshot_run_controls() {
        if [ -d /run/cortexfs/control ]; then
            "${system_privilege[@]}" find /run/cortexfs/control \
                -mindepth 1 -maxdepth 1 -printf '%f\n' 2>/dev/null
        fi | LC_ALL=C sort
    }
    wait_transient_service_idle() {
        local unit=$1
        local state
        for _attempt in $(seq 1 100); do
            state=$("${systemctl_system[@]}" is-active \
                "$unit.service" 2>/dev/null || true)
            case "$state" in
              active | activating | deactivating) sleep 0.1 ;;
              *) return 0 ;;
            esac
        done
        printf 'transient service did not become idle: %s.service\n' \
            "$unit" >&2
        return 1
    }
    assert_transient_runtime_clean() {
        local unit=$1
        local socket=$2
        local runtime_root=$3
        local control_before=$4
        local control_after=$5
        local label=$6
        local loaded_units
        local main_pid
        "${systemctl_system[@]}" stop \
            "$unit.socket" >/dev/null 2>&1 || true
        "${systemctl_system[@]}" stop \
            "$unit.service" >/dev/null 2>&1 || true
        "${systemctl_system[@]}" reset-failed \
            "$unit.socket" "$unit.service" >/dev/null 2>&1 || true
        wait_transient_service_idle "$unit"
        loaded_units=
        for _attempt in $(seq 1 100); do
            loaded_units=$("${systemctl_system[@]}" list-units \
                "$unit.socket" "$unit.service" \
                --all --no-legend --no-pager 2>/dev/null || true)
            if [ -z "$loaded_units" ]; then
                break
            fi
            sleep 0.1
        done
        if [ -n "$loaded_units" ]; then
            printf 'transient %s units survived cleanup:\n%s\n' \
                "$label" "$loaded_units" >&2
            return 1
        fi
        for _attempt in $(seq 1 100); do
            if [ ! -e "$socket" ]; then
                break
            fi
            sleep 0.1
        done
        if [ -e "$socket" ]; then
            printf 'transient %s socket survived cleanup: %s\n' \
                "$label" "$socket" >&2
            return 1
        fi
        main_pid=$("${systemctl_system[@]}" show \
            --property=MainPID --value "$unit.service" 2>/dev/null || true)
        case "$main_pid" in
          '' | 0) ;;
          *)
            printf 'transient %s service still has pid %s\n' \
                "$label" "$main_pid" >&2
            return 1
            ;;
        esac
        if pgrep -af '[c]ortexfs-agent-runtime' |
          grep -Fq -- "--source $runtime_root"; then
            printf 'worktree %s runtime process survived cleanup: %s\n' \
                "$label" "$runtime_root" >&2
            return 1
        fi
        snapshot_run_controls >"$control_after"
        if ! cmp -s "$control_before" "$control_after"; then
            printf 'run-control residue changed during %s smoke:\n' "$label" >&2
            diff -u "$control_before" "$control_after" >&2 || true
            return 1
        fi
    }
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
  grep -q 'self-bootstrap dirty note' "$user_dirty_file"
  local uid overlay_root overlay upper work
  uid=$(id -u)
  overlay_root="$root/home/$uid/agent/coder/session/smoke/workspace-overlay"
  overlay=$(find "$overlay_root" -mindepth 1 -maxdepth 1 -type d | head -n 1 || true)
  if [ -z "$overlay" ]; then
    printf 'missing workspace overlay under %s\n' "$overlay_root" >&2
    exit 1
  fi
  upper="$overlay/upper"
  work="$overlay/work"
  test -f "$upper/crates/cortexfs/tests/self_bootstrap_smoke.rs"
  local rustup_home cargo_home rustup_toolchain
  rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"
  cargo_home="${CARGO_HOME:-$HOME/.cargo}"
  rustup_toolchain=$(rustup show active-toolchain | awk '{print $1}')
  bwrap \
    --clearenv \
    --die-with-parent \
    --unshare-pid \
    --unshare-net \
    --proc /proc \
    --dev /dev \
    --size "$CORTEXFS_DEFAULT_SANDBOX_TMPFS_BYTES" --tmpfs /tmp \
    --ro-bind /usr /usr \
    --ro-bind /etc /etc \
    --dir /home \
    --dir "$HOME" \
    --ro-bind "$rustup_home" "$rustup_home" \
    --ro-bind "$cargo_home" "$cargo_home" \
    --symlink usr/bin /bin \
    --symlink usr/lib /lib \
    --symlink usr/lib /lib64 \
    --dir /workspace \
    --overlay-src "$workspace" \
    --overlay "$upper" "$work" /workspace \
    --chdir /workspace \
    --setenv PATH /usr/bin:/bin \
    --setenv RUSTUP_HOME "$rustup_home" \
    --setenv CARGO_HOME "$cargo_home" \
    --setenv RUSTUP_TOOLCHAIN "$rustup_toolchain" \
    -- /bin/sh -eu -c '
      grep -q self_bootstrap_agent_updates_cortexfs_source crates/cortexfs/tests/self_bootstrap_smoke.rs
      test -f Cargo.toml
      test -f crates/cortexfs/Cargo.toml
      rustc --test crates/cortexfs/tests/self_bootstrap_smoke.rs -o /tmp/self_bootstrap_smoke
      /tmp/self_bootstrap_smoke >/dev/null
      grep -q "self-bootstrap dirty note" README.md
      subject=$(git log -1 --format=%s)
      if [ "$subject" != "test: add self bootstrap smoke proof" ]; then
        printf "unexpected workspace git commit subject: %s\n" "$subject" >&2
        exit 1
      fi
      committed_files=$(git show --format= --name-only --stat HEAD)
      case "$committed_files" in
        *crates/cortexfs/tests/self_bootstrap_smoke.rs*) ;;
        *) printf "unexpected workspace git commit files:\n%s\n" "$committed_files" >&2; exit 1 ;;
      esac
      case "$committed_files" in
        *README.md*) printf "unexpected README commit:\n%s\n" "$committed_files" >&2; exit 1 ;;
      esac
      status=$(git status --short)
      case "$status" in
        *" M README.md"*) ;;
        *) printf "unexpected workspace git status after source improvement:\n%s\n" "$status" >&2; exit 1 ;;
      esac
      case "$status" in
        *"?? crates/cortexfs/tests/self_bootstrap_smoke.rs"*) printf "unexpected untracked smoke proof:\n%s\n" "$status" >&2; exit 1 ;;
      esac
    '
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
        assert_output_contains() {
            local needle=$1
            if ! grep -Fq "$needle" <<<"$output"; then
                printf 'missing final output evidence: %s\nfull output:\n%s\n' "$needle" "$output" >&2
                exit 1
            fi
        }
        assert_output_contains 'changed: crates/cortexfs/tests/self_bootstrap_smoke.rs'
        assert_output_contains "verified: $agent_verify_command"
        assert_output_contains 'diff: git -C /workspace diff --stat && git -C /workspace diff -- crates/cortexfs/tests/self_bootstrap_smoke.rs'
        assert_output_contains 'commit: git -C /workspace add crates/cortexfs/tests/self_bootstrap_smoke.rs && git -C /workspace commit -m'
        assert_output_contains 'committed: test: add self bootstrap smoke proof'
    }

    host_runner="$repo_root/target/debug/cortexfs-object-runner"
    host_tsh="$repo_root/target/debug/tsh"
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
    grep -Fq 'current workspace state' "$root/agent/coder.d/system.md"
    grep -Fq 'applicable `/workspace` rules' "$root/agent/coder.d/system.md"
    if grep -Fq 'git status --short' "$root/agent/coder.d/system.md"; then
        printf 'bootstrap system prompt embeds a git status command\n' >&2
        exit 1
    fi
    if grep -Fq 'find /workspace -name AGENTS.md -print' "$root/agent/coder.d/system.md"; then
        printf 'bootstrap system prompt embeds an AGENTS.md discovery command\n' >&2
        exit 1
    fi
    grep -Fq 'real build, test, and git evidence' "$root/agent/coder.d/system.md"
    grep -Fq 'Never run destructive git commands' "$root/agent/coder.d/system.md"
    write_self_bootstrap_workspace

    mkdir -p "$root/model/debug" "$root/model/debug/selfedit.d"
cat >"$root/model/debug/selfedit" <<'EOF'
#!/bin/sh
case "${CTX_AGENT_TOOL_CONTEXT:-}" in
    *"call-5"*)
    printf '{"type":"delta","run":"%s","text":"source self-improvement smoke complete\nchanged: crates/cortexfs/tests/self_bootstrap_smoke.rs\nverified: test -f /workspace/Cargo.toml && test -f /workspace/crates/cortexfs/Cargo.toml && rustc --test /workspace/crates/cortexfs/tests/self_bootstrap_smoke.rs -o /tmp/self_bootstrap_smoke && /tmp/self_bootstrap_smoke\ndiff: git -C /workspace diff --stat && git -C /workspace diff -- crates/cortexfs/tests/self_bootstrap_smoke.rs\ncommit: git -C /workspace add crates/cortexfs/tests/self_bootstrap_smoke.rs && git -C /workspace commit -m \"test: add self bootstrap smoke proof\"\ncommitted: test: add self bootstrap smoke proof"}\n' "$CTX_RUN_ID"
    printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
    ;;
  *"call-4"*)
    printf '{"type":"tool_call","run":"%s","id":"call-5","name":"tsh","arguments":{"args":["shell.exec","git -C /workspace add crates/cortexfs/tests/self_bootstrap_smoke.rs && git -C /workspace commit -m \\"test: add self bootstrap smoke proof\\""]}}\n' "$CTX_RUN_ID"
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
            printf '{"type":"tool_call","run":"%s","id":"call-1","name":"tsh","arguments":{"args":["shell.exec","find /workspace -name AGENTS.md -print; cat /workspace/AGENTS.md; if test -f /workspace/crates/cortexfs/src/AGENTS.md; then cat /workspace/crates/cortexfs/src/AGENTS.md; fi; cat /workspace/docs/DESIGN.md; git -C /workspace status --short"]}}\n' "$CTX_RUN_ID"
    ;;
  "")
            printf '{"type":"tool_call","run":"%s","id":"call-1","name":"tsh","arguments":{"args":["shell.exec","find /workspace -name AGENTS.md -print; cat /workspace/AGENTS.md; if test -f /workspace/crates/cortexfs/src/AGENTS.md; then cat /workspace/crates/cortexfs/src/AGENTS.md; fi; cat /workspace/docs/DESIGN.md; git -C /workspace status --short"]}}\n' "$CTX_RUN_ID"
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

    printf '%s\n' \
      'skipping source self-improvement smoke: legacy object-runner needs a production RunCapability-backed direct harness' >&2

    if [ "$systemd_fixture_ready" = true ] &&
      command -v nc >/dev/null 2>&1; then
        approval_nonce="$(id -u)-$$-$RANDOM"
        approval_root=$(mktemp -d)
        approval_unit="cortexfs-approval-smoke-$approval_nonce"
        approval_socket="$approval_root/agent/example-echo.sock"
        approval_uid=$(id -u)
        approval_home="$approval_root/home/$approval_uid/agent/example-echo/session"
        control_before="$approval_root/control.before"
        control_after="$approval_root/control.after"

        wait_approval_session_done() {
            local session_dir=$1
            for _attempt in $(seq 1 100); do
                if [ -f "$session_dir/state" ] &&
                  grep -qx 'done' "$session_dir/state"; then
                    return 0
                fi
                sleep 0.1
            done
            printf 'approval session did not finish: %s\n' "$session_dir" >&2
            return 1
        }
        assert_line_count() {
            local expected=$1
            local file=$2
            local first=$3
            local second=$4
            local label=$5
            local actual
            actual=$(awk -v first="$first" -v second="$second" \
                'index($0, first) && index($0, second) { count++ } END { print count + 0 }' \
                "$file")
            if [ "$actual" -ne "$expected" ]; then
                printf 'unexpected %s count: expected=%s actual=%s file=%s\n' \
                    "$label" "$expected" "$actual" "$file" >&2
                sed -n '1,200p' "$file" >&2
                return 1
            fi
        }
        assert_approval_call_facts() {
            local events=$1
            local messages=$2
            local call=$3
            assert_line_count 1 "$events" "\"id\":\"$call\"" \
                '"type":"approval_request"' "$call approval request"
            assert_line_count 1 "$events" "\"id\":\"$call\"" \
                '"type":"approval_result"' "$call approval result"
            assert_line_count 1 "$events" "\"tool_call_id\":\"$call\"" \
                '"type":"tool_result"' "$call event tool result"
            assert_line_count 1 "$messages" "\"tool_call_id\":\"$call\"" \
                '"type":"tool_result"' "$call message tool result"
        }
        assert_approval_session_facts() {
            local session=$1
            local decision=$2
            local marker=$3
            local expect_execution=$4
            local session_dir="$approval_home/$session"
            local events="$session_dir/events.jsonl"
            local messages="$session_dir/messages.jsonl"
            wait_approval_session_done "$session_dir"
            assert_approval_call_facts "$events" "$messages" echo-call-1
            assert_approval_call_facts "$events" "$messages" echo-call-2
            assert_line_count 2 "$events" '"type":"approval_result"' \
                "\"decision\":\"$decision\"" "$session $decision results"
            if [ "$decision" = deny ]; then
                assert_line_count 0 "$events" '"type":"approval_result"' \
                    '"decision":"allow_once"' "$session unexpected allows"
            fi
            if [ "$expect_execution" = yes ]; then
                assert_line_count 1 "$events" '"type":"tool_result"' \
                    "\"content\":\"$marker\"" "$session first execution"
                assert_line_count 1 "$events" '"type":"tool_result"' \
                    "\"content\":\"second:$marker\"" "$session second execution"
                grep -Fxq "second:$marker" "$session_dir/latest.md"
            else
                assert_line_count 0 "$events" '"type":"tool_result"' \
                    "$marker" "$session denied execution marker"
            fi
        }
        assert_policy_denied_session_facts() {
            local session=$1
            local marker=$2
            local session_dir="$approval_home/$session"
            local events="$session_dir/events.jsonl"
            local messages="$session_dir/messages.jsonl"
            wait_approval_session_done "$session_dir"
            assert_line_count 0 "$events" '"type":"approval_request"' \
                '' "$session unexpected approval requests"
            assert_line_count 0 "$events" '"type":"approval_result"' \
                '' "$session unexpected approval results"
            for call in echo-call-1 echo-call-2; do
                assert_line_count 1 "$events" "\"tool_call_id\":\"$call\"" \
                    '"type":"tool_result"' "$call policy-denied event result"
                assert_line_count 1 "$messages" "\"tool_call_id\":\"$call\"" \
                    '"type":"tool_result"' "$call policy-denied message result"
            done
            assert_line_count 0 "$events" '"type":"tool_result"' \
                "$marker" "$session policy-denied execution marker"
        }

        "$repo_root/target/debug/ctx" bootstrap "$approval_root" >/dev/null
        cp "$host_runner" "$approval_root/bin/cortexfs-object-runner"
        cp "$host_tsh" "$approval_root/bin/tsh"
        chmod 755 \
            "$approval_root/bin/cortexfs-object-runner" \
            "$approval_root/bin/tsh"
        CARGO_BUILD_JOBS=1 \
          CTX_BIN="$repo_root/target/debug/ctx" \
          CTX_SOURCE="$approval_root" \
          CORTEXFS_EXTENSION_INSTALL=yes \
          "$repo_root/examples/extensions/echo/install.sh" >/dev/null

        printf 'ask\n' >"$approval_root/agent/example-echo.d/approval"
        printf '%s/tool\n' "$approval_root" \
            >"$approval_root/agent/example-echo.d/path"
        printf '%s\t%s\tro\trbind,nosuid,nodev\n%s\t/ctx\tro\trbind,nosuid,nodev\n' \
            "$approval_root" "$approval_root" "$approval_root" \
            >"$approval_root/agent/example-echo.d/mount"
        echo_policy="$approval_root/tool/example.echo.d/policy"
        if [ ! -f "$echo_policy" ] || [ ! -r "$echo_policy" ]; then
            printf 'echo tool policy is missing, non-regular, or unreadable: %s\n' \
                "$echo_policy" >&2
            exit 1
        fi
        if grep -q '[^[:space:]]' "$echo_policy"; then
            echo_policy_status=0
        else
            echo_policy_status=$?
        fi
        case "$echo_policy_status" in
          0)
            printf 'unexpected non-whitespace echo tool policy after install:\n' >&2
            sed -n '1,20p' "$echo_policy" >&2
            exit 1
            ;;
          1) ;;
          *)
            printf 'cannot inspect echo tool policy (grep status %s): %s\n' \
                "$echo_policy_status" "$echo_policy" >&2
            exit 1
            ;;
        esac
        snapshot_run_controls >"$control_before"

        if ! "${systemd_run_system[@]}" \
          --unit="$approval_unit" \
          --collect \
          --property=NoNewPrivileges=yes \
          --property=RuntimeMaxSec=30s \
          --socket-property="ListenStream=$approval_socket" \
          --socket-property=SocketMode=0666 \
          --socket-property=RemoveOnStop=yes \
          "$repo_root/target/debug/cortexfs-agent-runtime" \
          --source "$approval_root" \
          --agent example-echo \
          >"$approval_root/systemd-run.log" 2>&1; then
            sed -n '1,200p' "$approval_root/systemd-run.log" >&2
            exit 1
        fi
        test -S "$approval_socket"
        "${systemctl_system[@]}" is-active --quiet "$approval_unit.socket"

        policy_marker="policy-$approval_nonce"
        if ! NO_COLOR=1 "$repo_root/target/debug/ctx" \
          --root "$approval_root" \
          agent send example-echo \
          --session policy-deny \
          --approve example.echo \
          "tool $policy_marker" \
          >"$approval_root/policy-deny.out" 2>"$approval_root/policy-deny.err"; then
            sed -n '1,200p' "$approval_root/policy-deny.err" >&2
            exit 1
        fi
        wait_transient_service_idle "$approval_unit"
        assert_policy_denied_session_facts policy-deny "$policy_marker"

        printf 'allow example_t tool:example.echo execute\n' \
            >"$approval_root/tool/example.echo.d/policy"

        allow_marker="allow-$approval_nonce"
        if ! NO_COLOR=1 "$repo_root/target/debug/ctx" \
          --root "$approval_root" \
          agent send example-echo \
          --session allow \
          --approve example.echo \
          "tool $allow_marker" \
          >"$approval_root/allow.out" 2>"$approval_root/allow.err"; then
            sed -n '1,200p' "$approval_root/allow.err" >&2
            exit 1
        fi
        wait_transient_service_idle "$approval_unit"
        assert_approval_session_facts allow allow_once "$allow_marker" yes

        nonmatch_marker="nonmatch-$approval_nonce"
        if ! NO_COLOR=1 "$repo_root/target/debug/ctx" \
          --root "$approval_root" \
          agent send example-echo \
          --session nonmatch \
          --approve other.tool \
          "tool $nonmatch_marker" \
          >"$approval_root/nonmatch.out" 2>"$approval_root/nonmatch.err"; then
            sed -n '1,200p' "$approval_root/nonmatch.err" >&2
            exit 1
        fi
        wait_transient_service_idle "$approval_unit"
        assert_approval_session_facts nonmatch deny "$nonmatch_marker" no

        omitted_marker="omitted-$approval_nonce"
        if ! NO_COLOR=1 "$repo_root/target/debug/ctx" \
          --root "$approval_root" \
          agent send example-echo \
          --session omitted \
          "tool $omitted_marker" \
          >"$approval_root/omitted.out" 2>"$approval_root/omitted.err"; then
            sed -n '1,200p' "$approval_root/omitted.err" >&2
            exit 1
        fi
        wait_transient_service_idle "$approval_unit"
        assert_approval_session_facts omitted deny "$omitted_marker" no

        disconnect_marker="disconnect-$approval_nonce"
        disconnect_run="disconnect-run-$approval_nonce"
        printf '{"op":"send","id":"%s","session":"disconnect","scope":"private","cwd":"/workspace","input":"tool %s"}\n' \
            "$disconnect_run" "$disconnect_marker" |
          nc -U -q 0 "$approval_socket" >/dev/null 2>&1
        wait_approval_session_done "$approval_home/disconnect"
        wait_transient_service_idle "$approval_unit"
        assert_approval_session_facts disconnect deny "$disconnect_marker" no

        assert_transient_runtime_clean \
            "$approval_unit" "$approval_socket" "$approval_root" \
            "$control_before" "$control_after" approval
        printf 'systemd socket approval smoke passed\n' >&2
        "${system_privilege[@]}" rm -rf "$approval_root"
        approval_root=
        approval_unit=
        approval_socket=
    else
        if [ "$systemd_fixture_ready" = true ]; then
            approval_skip_reason='nc unavailable'
        else
            approval_skip_reason=$systemd_skip_reason
        fi
        printf 'skipping systemd socket approval smoke: %s\n' \
            "$approval_skip_reason" >&2
    fi

    printf '%s\n' \
      'skipping installed source self-improvement smoke: legacy object-runner tool facts are not host-owned' >&2
else
    printf 'skipping bwrap source self-improvement smoke: bwrap not found\n' >&2
fi

printf 'agent tool-loop smoke passed\n'
