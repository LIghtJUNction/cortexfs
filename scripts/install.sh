#!/bin/sh
set -eu

REPOSITORY="LIghtJUNction/cortexfs"
REF=${CORTEXFS_REF:-main}

fail() {
    printf 'CortexFS installer: %s\n' "$1" >&2
    exit 1
}

[ "$(uname -s 2>/dev/null || true)" = "Linux" ] ||
    fail "Linux with systemd is required; macOS and Windows are not supported."
if [ ! -r /dev/tty ] || [ ! -w /dev/tty ]; then
    fail "a controlling terminal is required for explicit confirmation."
fi
command -v bash >/dev/null 2>&1 || fail "bash is required."

case "$0" in
    */install.sh)
        SCRIPT_DIR=$(CDPATH='' cd -P -- "$(dirname -- "$0")" && pwd) ||
            fail "cannot resolve the script directory."
        SOURCE_DIR=$(CDPATH='' cd -P -- "$SCRIPT_DIR/.." && pwd) ||
            fail "cannot resolve the source directory."
        if [ -f "$SOURCE_DIR/Cargo.toml" ] &&
            [ -f "$SOURCE_DIR/scripts/install-linux.sh" ]; then
            exec bash "$SOURCE_DIR/scripts/install-linux.sh" --source "$SOURCE_DIR"
        fi
        ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required to download CortexFS."
command -v tar >/dev/null 2>&1 || fail "tar is required to unpack CortexFS."
command -v mktemp >/dev/null 2>&1 || fail "mktemp is required."
case "$REF" in
    "" | -* | *..* | *[!A-Za-z0-9._/-]*) fail "CORTEXFS_REF is invalid." ;;
esac

ARCHIVE_URL="https://github.com/$REPOSITORY/archive/$REF.tar.gz"
printf '\nCortexFS · Linux source installer\n'
printf '  Download / 下载: %s\n' "$ARCHIVE_URL"
printf '  This downloads one repository snapshot, then starts its reviewed installer.\n'
printf '  将下载一个完整仓库快照，然后启动其中的安装器。\n\n'
printf 'Type DOWNLOAD CORTEXFS to continue / 输入 DOWNLOAD CORTEXFS 继续: ' >/dev/tty
IFS= read -r ANSWER </dev/tty || fail "confirmation was not received."
[ "$ANSWER" = "DOWNLOAD CORTEXFS" ] || fail "confirmation did not match; nothing changed."

TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/cortexfs-install.XXXXXX") ||
    fail "cannot create a temporary directory."
cleanup() {
    rm -rf -- "$TMP_DIR"
}
trap cleanup EXIT HUP INT TERM
mkdir "$TMP_DIR/source"

printf '→ Downloading repository snapshot...\n'
curl -fL --retry 3 --connect-timeout 15 -o "$TMP_DIR/source.tar.gz" "$ARCHIVE_URL"
printf '→ Unpacking snapshot...\n'
tar -xzf "$TMP_DIR/source.tar.gz" -C "$TMP_DIR/source"
set -- "$TMP_DIR/source"/*
if [ "$#" -ne 1 ] || [ ! -d "$1" ]; then
    fail "the repository archive did not contain one source directory."
fi
if [ ! -f "$1/Cargo.toml" ] || [ ! -f "$1/scripts/install-linux.sh" ]; then
    fail "the downloaded snapshot is incomplete."
fi

bash "$1/scripts/install-linux.sh" --source "$1"
