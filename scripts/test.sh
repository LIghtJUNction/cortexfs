#!/bin/sh
set -eu

if [ "$#" -eq 0 ]; then
  printf '%s\n' 'usage: scripts/test.sh COMMAND [ARG...]' >&2
  exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
. "$script_dir/resources.sh"
limit=${CORTEXFS_TEST_TMPFS_BYTES:-$CORTEXFS_DEFAULT_SANDBOX_TMPFS_BYTES}
case "$limit" in
  '' | *[!0-9]*)
    printf '%s\n' 'CORTEXFS_TEST_TMPFS_BYTES must be a byte count' >&2
    exit 2
    ;;
esac

workspace=$(pwd -P)
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
