# Build-free shell agent

This example packages a distinct hosted agent without Cargo. It implements the
same step-0 `sdk-envelope-v1` boundary as the Rust Agent SDK using POSIX shell
plus `jq`: all core keys, optional event/origin, no unknown envelope keys,
forward-compatible origin extension fields, exact framing,
a 1 MiB invocation frame, a 256 KiB response frame (including newline), bounded
contexts, and validated present event/origin values.
CortexFS still owns launch validation, sessions, policy,
lifecycle, and socket framing.

```bash
CTX_BIN=ctx ./install.sh                         # validation only
CORTEXFS_EXTENSION_INSTALL=yes \
CTX_SOURCE=/var/lib/cortexfs/storage/current \
  ./install.sh                                   # explicit installation
ctx agent start field-notes --session demo --cwd /workspace
ctx agent send field-notes --session demo "record the release decision"
```

Expected assistant content starts with `FieldNotes[demo]:`. Copy the example
and change the `FieldNotes` fallback in `agent.sh` before packaging to create a
different identity; do not assume arbitrary host environment variables cross
the sandbox boundary. Do not parse provider credentials or call provider APIs
from this script.
Published packages should pin every member hash and use
`ctx install --require-hashes`.

The example intentionally supports only step `0` and emits one canonical
`message` frame. Use `cortexfs-agent-sdk` when an agent needs tool continuation,
child handoff, richer events, or typed error handling.
