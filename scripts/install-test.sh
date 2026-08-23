#!/usr/bin/env bash
# shellcheck disable=SC1091,SC2031,SC2317,SC2329 # Dynamic sources, subshell globals, and test doubles are intentional.
set -Eeuo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

export CORTEXFS_INSTALL_LIB=1
# shellcheck source=install-linux.sh
source "$ROOT/scripts/install-linux.sh"
setup_style
TEST_TEMP=$(mktemp -d "${TMPDIR:-/tmp}/cortexfs-install-test.XXXXXX")
trap 'rm -rf -- "$TEST_TEMP"' EXIT HUP INT TERM

PASSED=0
assert_eq() {
    local expected=$1 actual=$2 label=$3
    if [[ $actual != "$expected" ]]; then
        printf 'not ok - %s (expected %q, got %q)\n' "$label" "$expected" "$actual" >&2
        exit 1
    fi
    ((++PASSED))
    printf 'ok - %s\n' "$label"
}

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

fixture() {
    local name=$1
    shift
    printf '%s\n' "$@" >"$TEST_TEMP/$name"
    printf '%s\n' "$TEST_TEMP/$name"
}

assert_eq zh "$(locale_language zh_CN.UTF-8)" "Chinese locale defaults to zh"
assert_eq en "$(locale_language en_US.UTF-8)" "English locale defaults to en"
assert_true "bubblewrap 0.10 meets 0.10" version_ge 0.10.0 0.10.0
assert_false "bubblewrap 0.9 is below 0.10" version_ge 0.9.0 0.10.0
assert_true "Rust 1.91 meets MSRV" version_ge 1.91.0 1.91.0
assert_false "Rust 1.90 is below MSRV" version_ge 1.90.9 1.91.0
assert_true "newer prerelease numeric core is accepted" version_ge 1.92.0-beta.1 1.91.0

arch=$(fixture arch 'ID=endeavouros' 'ID_LIKE="arch"')
debian=$(fixture debian 'ID=ubuntu' 'ID_LIKE=debian')
fedora=$(fixture fedora 'ID=rocky' 'ID_LIKE="rhel fedora"')
suse=$(fixture suse 'ID="opensuse-tumbleweed"' 'ID_LIKE="opensuse suse"')
unknown=$(fixture unknown 'ID=alpine')
assert_eq 'arch|pacman|endeavouros' "$(detect_distro "$arch" pacman)" "Arch-family mapping"
assert_eq 'debian|apt-get|ubuntu' "$(detect_distro "$debian" apt-get)" "Debian-family mapping"
assert_eq 'fedora|dnf|rocky' "$(detect_distro "$fedora" dnf)" "Fedora-family mapping"
assert_eq 'suse|zypper|opensuse-tumbleweed' "$(detect_distro "$suse" zypper)" "SUSE-family mapping"
assert_false "family is rejected without its real package manager" detect_distro "$debian" pacman
assert_false "unknown distro is rejected" detect_distro "$unknown" "pacman apt-get dnf zypper"

good_state=$(fixture good-state 'schema=1' 'language=zh')
# shellcheck disable=SC2016 # Deliberately hostile literal state content.
bad_state=$(fixture bad-state 'schema=1' 'language=$(touch /tmp/never-cortexfs)')
extra_state=$(fixture extra-state 'schema=1' 'language=en' 'extra=yes')
assert_eq zh "$(read_state_language "$good_state")" "valid persisted language"
assert_false "malicious state text is rejected" read_state_language "$bad_state"
assert_false "extra state keys are rejected" read_state_language "$extra_state"

assert_eq first "$(classify_install no no)" "new host is first install"
assert_eq existing "$(classify_install no yes)" "manual install is an upgrade"
assert_eq managed "$(classify_install yes yes)" "state marker identifies managed rerun"
assert_eq managed "$(classify_install yes no)" "marker prevents false first-install label"

assert_true "exact confirmation accepts exact token" exact_match 'DEPLOY CORTEXFS' 'DEPLOY CORTEXFS'
assert_false "exact confirmation rejects whitespace" exact_match ' DEPLOY CORTEXFS' 'DEPLOY CORTEXFS'
export CORTEXFS_INSTALL_TEST_MODE=1 CORTEXFS_TEST_INPUT='TEST TOKEN'
assert_true "library-only confirmation override accepts exact test input" \
    confirm 'TEST TOKEN' test test
rejected_confirmation() (
    CORTEXFS_TEST_INPUT=WRONG confirm 'TEST TOKEN' test test
)
assert_false "library-only confirmation override rejects a mismatch" \
    rejected_confirmation
unset CORTEXFS_INSTALL_TEST_MODE CORTEXFS_TEST_INPUT
export CORTEXFS_ASSUME_YES=1
TTY_PATH="$TEST_TEMP/missing-tty"
assert_true "explicit updater approval bypasses nested installer prompts" \
    confirm 'NEVER READ' test test
unset CORTEXFS_ASSUME_YES
TTY_PATH=/dev/tty

assert_true "Linux systemd host is accepted" platform_supported Linux systemd yes
assert_false "non-Linux host is rejected" platform_supported Darwin launchd no
assert_false "non-systemd Linux is rejected" platform_supported Linux init yes
assert_false "Linux without systemd runtime is rejected" platform_supported Linux systemd no

paths=$(runtime_paths)
for required in /usr/bin/bwrap /usr/bin/setpriv /usr/bin/setsid /usr/bin/systemctl \
    /usr/bin/systemd-run /usr/bin/curl /usr/bin/env /usr/bin/id /usr/bin/sh \
    /usr/bin/findmnt /usr/bin/umount /usr/bin/install; do
    assert_true "runtime path audit includes $required" grep -Fxq "$required" <<<"$paths"
done

sudo_log="$TEST_TEMP/sudo.log"
mount_state=mounted
sudo() {
    printf '%s\n' "$*" >>"$sudo_log"
}
findmnt() {
    [[ $* == '-rnM /ctx' && $mount_state == mounted ]]
}
ensure_mountpoint
assert_false "mounted rerun does not touch /ctx" grep -Fq /ctx "$sudo_log"
mount_state=missing
ensure_mountpoint
assert_true "unmounted install creates /ctx" grep -Fxq 'install -d -m 0755 /ctx' "$sudo_log"
unset -f sudo findmnt

source_file="$TEST_TEMP/source"
destination="$TEST_TEMP/destination"
unrelated="$TEST_TEMP/unrelated"
printf 'release-v1\n' >"$source_file"
printf 'keep\n' >"$unrelated"
sudo() {
    "$@"
}
ROOT_TEMP_FILES=()
: "${ROOT_TEMP_FILES[@]}"
atomic_install "$source_file" "$destination" 0755
assert_eq release-v1 "$(<"$destination")" "atomic install publishes the release"
atomic_install "$source_file" "$destination" 0755
assert_eq keep "$(<"$unrelated")" "idempotent install preserves unrelated state"

artifact_source="$TEST_TEMP/artifacts"
artifact_stage="$TEST_TEMP/stage"
mkdir -p "$artifact_source"
printf 'trusted\n' >"$artifact_source/file"
snapshots="$(sha256sum "$artifact_source/file" | awk '{print $1}') file"
stage_artifacts "$artifact_source" "$artifact_stage" "$snapshots"
assert_eq trusted "$(<"$artifact_stage/file")" "validated artifact is staged"
printf 'tampered\n' >"$artifact_source/file"
reject_changed_artifact() (
    stage_artifacts "$artifact_source" "$TEST_TEMP/rejected-stage" "$snapshots"
)
assert_false "changed artifact is rejected before deployment" reject_changed_artifact
assert_true "source deployment includes the host updater" \
    grep -Fxq scripts/update-linux.sh < <(artifact_paths)

STATE_FILE="$TEST_TEMP/state"
TEMP_DIR="$TEST_TEMP"
test -d "$TEMP_DIR"
LANGUAGE=zh
write_state
assert_eq zh "$(read_state_language "$STATE_FILE")" "state writer publishes safe persisted language"

secret_capture="$TEST_TEMP/secret"
sudo() {
    printf '%s\n' "$*" >>"$sudo_log"
    if [[ $* == "ctx provider secret set openai" ]]; then
        cat >"$secret_capture"
    fi
}
store_api_secret openai 'secret with spaces'
assert_eq 'secret with spaces' "$(<"$secret_capture")" "API secret reaches ctx only through stdin"
assert_false "API secret is absent from command log" grep -Fq 'secret with spaces' "$sudo_log"

: >"$sudo_log"
TTY_PATH="$TEST_TEMP/tty"
: >"$TTY_PATH"
export CORTEXFS_INSTALL_TEST_MODE=1 CORTEXFS_TEST_INPUT='CONFIGURE AI'
configure_codex
assert_true "Codex preset installation runs through sudo" \
    grep -Fxq 'ctx provider preset install codex' "$sudo_log"
assert_true "Codex OAuth login uses the root system store" \
    grep -Fxq 'ctx provider oauth login codex --device' "$sudo_log"
unset CORTEXFS_INSTALL_TEST_MODE CORTEXFS_TEST_INPUT
unset -f sudo

package_payload_lists_match() (
    export CORTEXFS_PACKAGE_LIB=1
    # shellcheck disable=SC1090,SC1091 # The package script is sourced only in this subshell.
    source "$ROOT/packaging/build.sh"
    diff -u <(printf '%s\n' "${BINARIES[@]}" | sort) <(expected_binaries | sort)
    diff -u <(printf '%s\n' "${UNITS[@]}" | sort) <(expected_units | sort)
)
assert_true "source and native package payload lists agree" package_payload_lists_match

assert_true "POSIX entrypoint syntax" sh -n "$ROOT/scripts/install.sh"
assert_true "Bash installer syntax" bash -n "$ROOT/scripts/install-linux.sh"
assert_true "Bash updater syntax" bash -n "$ROOT/scripts/update-linux.sh"
assert_true "Bash test syntax" bash -n "$ROOT/scripts/install-test.sh"

printf '1..%d\n' "$PASSED"
