#!/usr/bin/env sh
set -eu

if ! command -v verus >/dev/null 2>&1; then
    printf '%s\n' \
        'verus binary not found on PATH.' \
        'Install Verus from https://github.com/verus-lang/verus, then rerun scripts/verify-verus.sh.' >&2
    exit 127
fi

verus --crate-type=lib proofs/verus/abi_name.rs "$@"
