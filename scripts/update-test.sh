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
assert_false 'service startup retains the rollback generation' \
    grep -Fq -- 'storage update --prune' "$ROOT/packaging/systemd/cortexfs.service"
assert_true 'Debian package scripts support updater-owned restarts' \
    grep -Fq CORTEXFS_UPDATE_TRANSACTION "$ROOT/packaging/debian/postinst"
assert_true 'Arch package scripts support updater-owned restarts' \
    grep -Fq CORTEXFS_UPDATE_TRANSACTION "$ROOT/packaging/arch/cortexfs.install"

state=$TEST_TEMP/state
printf 'schema=1\nphase=prepared\n' >"$state"
assert_eq prepared "$(update_state_field phase "$state")" 'transaction state uses fixed key lookup'

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
