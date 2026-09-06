#!/bin/sh
set -eu

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
ctx_bin=${CTX_BIN:-ctx}
command -v jq >/dev/null 2>&1 || {
    echo "jq is required by the shell agent" >&2
    exit 127
}

"$ctx_bin" install --check "$here"
if [ "${CORTEXFS_EXTENSION_INSTALL:-}" != "yes" ]; then
    echo "package valid; set CORTEXFS_EXTENSION_INSTALL=yes to opt in to installation" >&2
    exit 0
fi
if [ -z "${CTX_SOURCE:-}" ]; then
    echo "set CTX_SOURCE to the durable CortexFS backing tree" >&2
    exit 2
fi

"$ctx_bin" install --source "$CTX_SOURCE" --tier system "$here"
