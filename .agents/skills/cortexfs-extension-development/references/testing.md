# Extension Acceptance

Run cheap, read-only checks before any install:

```bash
rtk cargo fmt --check
rtk cargo test -p cortexfs-tool-sdk
rtk cargo test -p cortexfs-agent-sdk
rtk cargo check --manifest-path examples/extensions/echo/Cargo.toml
rtk git diff --check
```

Use a temporary explicit `--root` fixture for installer integration tests.
Cover strict parsing, unknown-field and unknown-control rejection, symlink and
non-executable rejection, SHA-256 mismatch, absence of a visible executable on
failure, possible hidden `.cortexfs-install-*` safety residue, and collision
preservation with byte-for-byte comparisons.

Do not install into `/ctx` or another live root during default validation.
Perform live mutation only when explicitly requested. The canonical example
requires `CORTEXFS_EXTENSION_INSTALL=yes` and an explicit durable `CTX_SOURCE`
before its `install.sh` mutates the backing tree. `/ctx` is only a projection.

For live acceptance, verify in order:

1. Install the tool, then the agent; do not claim the pair is atomic.
2. Resolve the tool through the intended `CTX_PATH` tier.
3. Confirm the default-deny installed tool cannot be executed.
4. Add an explicit tool-policy and agent-policy allow through the normal
   control-plane workflow, then confirm the authorized agent receives ordered
   `start`, content/error, and `done` frames.
5. Send input to the custom agent through the supported runtime path and
   confirm ordered agent frames plus durable session history.
6. Confirm canonical `status`, `pid`, and `log` controls exist where required,
   and confirm the installer created no socket state.

Run broader workspace gates only at the integration owner’s request and with
the repository’s low-load policy.
