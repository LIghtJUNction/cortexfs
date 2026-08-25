#!/bin/sh
set -eu

script_dir=$(dirname "$0")
repo_root=$(cd -- "$script_dir/.." && pwd -P)
target_dir=${CARGO_TARGET_DIR:-"$repo_root/target"}
case "$target_dir" in
  /*) ;;
  *) target_dir="$repo_root/$target_dir" ;;
esac
lock_path=${CORTEXFS_RUSTC_LOCK:-"$target_dir/.cortexfs-rustc.lock"}
lock_dir=$(dirname "$lock_path")
mkdir -p "$lock_dir"

if ! command -v flock >/dev/null 2>&1; then
  printf '%s\n' 'error: flock is required to serialize CortexFS compiler invocations' >&2
  exit 127
fi

exec 9>"$lock_path"
flock -x 9
exec "$@"
