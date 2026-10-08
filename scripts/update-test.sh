#!/usr/bin/env bash
set -Eeuo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export CORTEXFS_UPDATE_LIB=1
# shellcheck source=scripts/update-linux.sh
source "$ROOT/scripts/update-linux.sh"
TEST_TEMP=$(mktemp -d "${TMPDIR:-/tmp}/cortexfs-update-test.XXXXXX")
trap 'rm -rf -- "$TEST_TEMP"' EXIT HUP INT TERM
PASSED=0

assert_true() {
    local label=$1
    shift
    "$@" || {
        printf 'not ok - %s\n' "$label" >&2
        exit 1
    }
    ((++PASSED))
    printf 'ok - %s\n' "$label"
}

assert_false() {
    local label=$1
    shift
    if "$@" >/dev/null 2>&1; then
        printf 'not ok - %s\n' "$label" >&2
        exit 1
    fi
    ((++PASSED))
    printf 'ok - %s\n' "$label"
}

assert_eq() {
    local expected=$1 actual=$2 label=$3
    [[ $actual == "$expected" ]] || {
        printf 'not ok - %s (expected %q, got %q)\n' "$label" "$expected" "$actual" >&2
        exit 1
    }
    ((++PASSED))
    printf 'ok - %s\n' "$label"
}

manifest_case() (
    local failure=$1
    local UPDATE_TEMP=$TEST_TEMP/manifest-$failure
    mkdir -p "$UPDATE_TEMP"
    local manifest=$UPDATE_TEMP/list
    {
        [[ $failure == missing-ctx ]] || printf '/usr/bin/ctx\n'
        [[ $failure == missing-updater ]] || printf './usr/lib/cortexfs/update-linux\n'
        [[ $failure != valid ]] || printf 'usr/share/doc/cortexfs/%s\n' {1..4096}
        [[ $failure != unmanaged ]] || printf '/etc/passwd\n'
    } >"$manifest"
    update_package_paths() { cat "$manifest"; [[ $failure != list-error ]]; }
    update_verify_package fixture
)

rollback_case() (
    local failure=$1
    local UPDATE_TEMP=$TEST_TEMP/rollback-$failure
    local UPDATE_TXN=$UPDATE_TEMP/transaction
    local UPDATE_OWNER=deb
    local UPDATE_BACKEND=deb
    [[ $failure != remove && $failure != unpack ]] || UPDATE_OWNER=source
    local UPDATE_SWITCHED=1
    mkdir -p "$UPDATE_TXN"
    printf 'cortexfs.service\n' >"$UPDATE_TEMP/active-units"
    touch "$UPDATE_TXN/pending" "$UPDATE_TXN/rollback.deb"
    printf 'installing\n' >"$UPDATE_TXN/phase"
    update_restore_storage() { [[ $failure != storage ]]; }
    update_install_package() { [[ $2 == "$UPDATE_TXN/rollback.deb" && $failure != install ]]; }
    update_restart_units() { [[ $failure != restart ]]; }
    update_write_txn_state() { [[ $failure != state ]] && printf '%s\n' "$1" >"$UPDATE_TXN/phase"; }
    sudo() {
        case "$1" in
        find) [[ $failure != find ]] && printf '%s\n' "$UPDATE_TXN/rollback.deb" ;;
        systemctl) [[ $failure != "$2" && ( $failure != reload || $2 != daemon-reload ) ]] ;;
        env) [[ $failure != remove ]] ;;
        tar) [[ $failure != unpack ]] ;;
        rm) [[ $failure != unlink ]] && rm -f "$UPDATE_TXN/pending" ;;
        *) return 1 ;;
        esac
    }
    if update_rollback; then
        [[ $failure == ok && ! -e $UPDATE_TXN/pending && $(<"$UPDATE_TXN/phase") == rolled-back ]] || return 1
    else
        [[ $failure != ok && -e $UPDATE_TXN/pending ]] || return 1
        [[ $failure == unlink || $(<"$UPDATE_TXN/phase") == installing ]] || return 1
    fi
    [[ $UPDATE_SWITCHED == 0 ]] || return 1
    failure=ok
    UPDATE_SWITCHED=1
    update_rollback && [[ $UPDATE_SWITCHED == 0 && ! -e $UPDATE_TXN/pending && $(<"$UPDATE_TXN/phase") == rolled-back ]]
)

extraction_failure_case() (
    local UPDATE_TEMP=$TEST_TEMP/extraction
    local UPDATE_OWNER=deb
    mkdir -p "$UPDATE_TEMP"
    dpkg-deb() { return 1; }
    update_package_matches_install missing.deb
)

helper_failure_case() (
    local failure=$1
    local UPDATE_TEMP=$TEST_TEMP/helper-$failure
    local UPDATE_TXN=$UPDATE_TEMP/transaction
    local UPDATE_OWNER=deb
    local UPDATE_BACKEND=deb
    local UPDATE_STORAGE_TARGET=generations/old
    mkdir -p "$UPDATE_TXN"
    : >"$UPDATE_TEMP/active-units"
    sudo() { [[ $1 != "$failure" && $2 != "$failure" ]]; }
    case "$failure" in
    ln) update_restore_storage ;;
    install) update_write_txn_state rolled-back ;;
    discovery)
        update_active_units() { return 1; }
        update_restart_units
        ;;
    stop)
        update_active_units() { printf 'cortexfs-extra.service\n'; }
        update_restart_units
        ;;
    esac
)

assert_true 'large package manifest is normalized and checked completely' manifest_case valid
for scenario in missing-ctx missing-updater unmanaged list-error; do
    assert_false "invalid package manifest: $scenario" manifest_case "$scenario"
done
for scenario in ok stop storage find install remove unpack restart reload state unlink; do
    assert_true "rollback preserves recovery until success: $scenario" rollback_case "$scenario"
done
assert_false 'rollback package extraction failure is rejected' extraction_failure_case
for scenario in ln install discovery stop; do
    assert_false "rollback helper propagates failure: $scenario" helper_failure_case "$scenario"
done

assert_true 'branch ref is accepted' update_valid_ref main
assert_true 'tag ref is accepted' update_valid_ref v0.1.20
assert_true 'full commit is accepted' update_valid_ref 0123456789012345678901234567890123456789
assert_false 'option-shaped ref is rejected' update_valid_ref --upload-pack=bad
assert_false 'whitespace ref is rejected' update_valid_ref 'main next'
assert_false 'newline ref is rejected' update_valid_ref $'main\nnext'

for path in usr/bin/ctx usr/bin/cortexfs-mount usr/lib/cortexfs/update-linux \
    usr/lib/systemd/system/cortexfs.service usr/share/doc/cortexfs/README.md \
    etc/cortexfs/channels var/lib/cortexfs/storage/generations; do
    assert_true "managed package path: $path" update_path_allowed "$path"
done
assert_true 'Arch package metadata is accepted' update_path_allowed .PKGINFO
assert_true 'RPM build-id links are accepted' update_path_allowed usr/lib/.build-id/aa/bb
assert_false 'package cannot write provider configuration' \
    update_path_allowed etc/cortexfs/providers.d/openai.toml
assert_false 'package cannot write storage contents' \
    update_path_allowed var/lib/cortexfs/storage/generations/active/bin/ctx
assert_false 'package cannot add unrelated executables' update_path_allowed usr/bin/curl
for path in ../usr/bin/ctx usr/share/doc/cortexfs/../../../../etc/passwd \
    usr/share/doc/cortexfs/.. usr/lib/.build-id/.. usr/bin/cortexfs-helper/..; do
    assert_false "package cannot traverse a parent: $path" update_path_allowed "$path"
done
assert_false 'service startup retains the rollback generation' \
    grep -Fq -- 'storage update --prune' "$ROOT/packaging/systemd/cortexfs.service"
assert_true 'Debian package scripts support updater-owned restarts' \
    grep -Fq CORTEXFS_UPDATE_TRANSACTION "$ROOT/packaging/debian/postinst"
assert_true 'Arch package scripts support updater-owned restarts' \
    grep -Fq CORTEXFS_UPDATE_TRANSACTION "$ROOT/packaging/arch/cortexfs.install"

state=$TEST_TEMP/state
printf 'schema=1\nphase=prepared\n' >"$state"
assert_eq prepared "$(update_state_field phase "$state")" 'transaction state uses fixed key lookup'

active_units_include_primary() (
    mock_bin=$TEST_TEMP/active-units-bin
    mkdir -p "$mock_bin"
    printf '%s\n' \
        '#!/bin/sh' \
        "printf '%s\\n' 'cortexfs.service loaded active running CortexFS' 'cortexfs-agent.service loaded active running CortexFS agent' 'cortexfs-agent@foo_bar.socket loaded active listening Custom agent' 'postgres.service loaded active running Other service' 'cortexfs-update.timer loaded active waiting Timer' 'cortexfs.service.evil loaded active running Invalid suffix'" \
        >"$mock_bin/systemctl"
    chmod +x "$mock_bin/systemctl"
    [[ $(PATH="$mock_bin:$PATH" update_active_units) == \
        "$(printf '%s\n' cortexfs.service cortexfs-agent.service cortexfs-agent@foo_bar.socket | sort -u)" ]]
)
assert_true 'active unit discovery includes primary and underscore instances only' active_units_include_primary

fixture=$TEST_TEMP/source
mkdir -p "$fixture/packaging" "$fixture/scripts"
printf '%s\n' "$UPDATE_PROTOCOL" >"$fixture/packaging/update-protocol"
printf '#!/bin/sh\n' >"$fixture/packaging/build.sh"
printf '#!/bin/bash\n' >"$fixture/scripts/install-linux.sh"
printf '#!/bin/bash\n' >"$fixture/scripts/update-linux.sh"
chmod +x "$fixture/packaging/build.sh" "$fixture/scripts/update-linux.sh"
git -C "$fixture" init --quiet
git -C "$fixture" config user.name test
git -C "$fixture" config user.email test@example.invalid
git -C "$fixture" add .
git -C "$fixture" commit --quiet -m fixture
revision=$(git -C "$fixture" rev-parse HEAD)
# shellcheck disable=SC2030
resolved=$(
    export UPDATE_SOURCE=$fixture UPDATE_REF='' UPDATE_TEMP=$TEST_TEMP/resolve
    update_resolve_target
    printf '%s' "$UPDATE_REVISION"
)
assert_eq "$revision" "$resolved" 'clean source resolves exactly HEAD'
printf 'dirty\n' >"$fixture/untracked"
dirty_source_is_rejected() (
    # shellcheck disable=SC2031
    export UPDATE_SOURCE=$fixture UPDATE_TEMP=$TEST_TEMP/dirty
    update_resolve_target
)
assert_false 'dirty source is rejected' dirty_source_is_rejected

assert_true 'uninitialized rustup shim is treated as missing Rust' \
    grep -Fq $'current=$(rust_version || true)' "$ROOT/scripts/install-linux.sh"
assert_false 'updater does not call an undefined Rust audit' \
    grep -Fq '    audit_rust' "$ROOT/scripts/update-linux.sh"
assert_true 'updater uses the defined bwrap check' \
    grep -Fq '    check_bwrap' "$ROOT/scripts/update-linux.sh"
assert_false 'updater does not call an undefined bwrap audit' \
    grep -Fq '    audit_bwrap' "$ROOT/scripts/update-linux.sh"
assert_true 'updater syntax' bash -n "$ROOT/scripts/update-linux.sh"
assert_true 'updater tests syntax' bash -n "$ROOT/scripts/update-test.sh"
printf '1..%d\n' "$PASSED"
