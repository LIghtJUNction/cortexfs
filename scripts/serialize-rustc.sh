#!/bin/sh
set -eu

script_dir=$(dirname "$0")
repo_root=$(cd -- "$script_dir/.." && pwd -P)
lock_path=${CORTEXFS_RUSTC_LOCK:-"$repo_root/target/.cortexfs-rustc.lock"}
lock_dir=$(dirname "$lock_path")
mkdir -p "$lock_dir"

if ! command -v flock >/dev/null 2>&1; then
  printf '%s\n' 'error: flock is required to serialize CortexFS compiler invocations' >&2
  exit 127
fi

exec 9>"$lock_path"
flock -x 9
exec "$@"
