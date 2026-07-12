#!/bin/sh
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ctx_bin=${CTX_BIN:-ctx}

cargo build --release --manifest-path "$here/Cargo.toml"
tool_sha=$(sha256sum "$here/target/release/cortexfs-echo-tool" | cut -d ' ' -f 1)
agent_sha=$(sha256sum "$here/target/release/cortexfs-echo-agent" | cut -d ' ' -f 1)
uid=$(id -u)
gid=$(id -g)
groups=$gid
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

sed -e "s|@TOOL_PATH@|$here/target/release/cortexfs-echo-tool|g" \
    -e "s/@TOOL_SHA256@/$tool_sha/g" "$here/tool.manifest.json.in" > "$tmp/tool.json"
sed -e "s|@AGENT_PATH@|$here/target/release/cortexfs-echo-agent|g" \
    -e "s/@AGENT_SHA256@/$agent_sha/g" \
    -e "s/@UID@/$uid/g" \
    -e "s/@GID@/$gid/g" \
    -e "s/@GROUPS@/$groups/g" \
    "$here/agent.manifest.json.in" > "$tmp/agent.json"

"$ctx_bin" object check "$tmp/tool.json"
"$ctx_bin" object check "$tmp/agent.json"

if [ "${CORTEXFS_EXTENSION_INSTALL:-}" != "yes" ]; then
    echo "manifests valid; set CORTEXFS_EXTENSION_INSTALL=yes to opt in to object installation" >&2
    exit 2
fi
if [ -z "${CTX_SOURCE:-}" ]; then
    echo "set CTX_SOURCE to the durable CortexFS backing tree" >&2
    exit 2
fi

"$ctx_bin" object install --source "$CTX_SOURCE" "$tmp/tool.json" --tier system
"$ctx_bin" object install --source "$CTX_SOURCE" "$tmp/agent.json" --tier system
