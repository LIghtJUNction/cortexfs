#!/bin/sh
set -eu

if [ "$#" -eq 0 ]; then
  printf '%s\n' 'usage: scripts/test.sh COMMAND [ARG...]' >&2
  exit 2
fi

script_dir=$(dirname "$0")
. "$script_dir/resources.sh"
limit=${CORTEXFS_TEST_TMPFS_BYTES:-$CORTEXFS_DEFAULT_SANDBOX_TMPFS_BYTES}
case "$limit" in
'' | *[!0-9]*)
  printf '%s\n' 'CORTEXFS_TEST_TMPFS_BYTES must be a byte count' >&2
  exit 2
  ;;
esac

workspace=$(pwd -P)
cargo_lock=${CORTEXFS_CARGO_LOCK:-"$workspace/target/.cortexfs-cargo.lock"}
mkdir -p "$(dirname "$cargo_lock")"
if ! command -v flock >/dev/null 2>&1; then
  printf '%s\n' 'error: flock is required to serialize CortexFS test invocations' >&2
  exit 127
fi
exec 9>"$cargo_lock"
flock -x 9
cargo_home=${CARGO_HOME:-"$HOME/.cargo"}
exec bwrap \
  --die-with-parent \
  --ro-bind /usr /usr \
  --ro-bind /etc /etc \
  --ro-bind /home /home \
  --bind "$workspace" "$workspace" \
  --symlink usr/bin /bin \
  --symlink usr/lib /lib \
  --symlink usr/lib /lib64 \
  --proc /proc \
  --dev /dev \
  --size "$limit" --tmpfs /tmp \
  --setenv TMPDIR /tmp \
  --setenv CARGO_HOME "$cargo_home" \
  --setenv PATH "$cargo_home/bin:/usr/bin:/bin" \
  --chdir "$workspace" \
  -- "$@"
