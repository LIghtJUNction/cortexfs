---
name: cortexfs-test
description: Use this skill when working in the CortexFS repository and the user asks to test CortexFS, test FUSE, test /ctx, test ctx, test agent.sh, run live tests, use Ollama or smollm2:135m, verify file/socket ABI behavior, check tests/mounts/cortexfs, inspect the systemd mount, update the AUR package, or diagnose mount/socket permission failures.
---

# CortexFS Test

Apply this project skill for CortexFS test, live-test, file API, socket API,
systemd mount, AUR package, and FUSE verification work inside this repository.

## Scope

- Treat Linux as the only supported runtime target.
- Read `docs/DESIGN.md` before changing ABI, FUSE behavior, provider/model
  projection, tests, or docs that describe filesystem semantics.
- Treat `/ctx` as an ABI, not a UI. Preserve documented path, read/write,
  permission, socket, and errno semantics.
- Keep the root ABI frozen and short:

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
```

## Ground Rules

- Do not add root namespaces such as `provider`, `format`, `mcp`, `skill`,
  `memory`, `vector`, `cluster`, `workflow`, `job`, `hook`, or `control`.
- Do not reintroduce old ABI checks such as `cap/format`, `provider/list`,
  `model/list`, or `home/<uid>/route/openai.chat`.
- Keep provider/model design neutral. Ollama is only a local live-test fixture,
  never a privileged provider, default ABI path, core capability, or special
  branch.
- Preserve the unified submission contract where it applies: write a temporary
  file, atomically rename it in the same directory to `*.req.json`, read
  outbox, and audit facts.
- Use Git commits as the only development refresh boundary. Do not add
  background watchers, polling, hot reload, or `dev` subcommands.
- Do not add `mod.rs`.
- Add Rust dependencies with `cargo add`; do not manually edit dependency
  entries.
- Keep `tests/mounts/cortexfs` as the local FUSE integration-test mountpoint.
- Keep that mountpoint empty except for the mounted virtual tree; do not place
  source files, fixtures, or durable state inside it.
- Avoid external cloud APIs in tests unless the user explicitly asks to test an
  already configured provider or local aggregation API.

## Standard Test Ladder

Run tests from cheap to expensive:

1. `cargo fmt --check`
2. `cargo check --locked --workspace --all-targets --all-features`
3. `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
4. `cargo test --locked --workspace --all-targets --all-features`
5. Targeted CLI smoke tests:

```bash
cargo run -p cortexfs --bin ctx -- status
cargo run -p cortexfs --bin ctx -- ls
cargo run -p cortexfs --bin ctx -- ls home
cargo run -p cortexfs --bin ctx -- doctor
```

6. Installed CLI smoke tests when `/usr/bin/ctx` is present:

```bash
/usr/bin/ctx status
/usr/bin/ctx ls
/usr/bin/ctx ls /
/usr/bin/ctx ls home
/usr/bin/ctx ls tool
/usr/bin/ctx doctor
```

7. FUSE mount tests under `tests/mounts/cortexfs`.
8. Live socket tests only when an actual runtime/listener exists.
9. Local Ollama checks for `smollm2:135m` only when live model testing is
   explicitly requested.

## Current System Mount Checks

The current packaged system mount is systemd-level, not a user daemon:

```text
/usr/lib/systemd/system/cortexfs.service
/usr/bin/cortexfs-mount --source /var/lib/cortexfs/storage/current /ctx
```

Check it with:

```bash
systemctl is-enabled cortexfs.service
systemctl is-active cortexfs.service
findmnt -no TARGET,SOURCE,FSTYPE,OPTIONS /ctx
/usr/bin/ctx status
/usr/bin/ctx doctor
```

Expected mount facts:

- `/ctx` is mounted as `fuse` with source `cortexfs`.
- Options include `allow_other` when non-root users must inspect `/ctx`.
- The service should be `enabled` and `active` after installation.
- A systemd restart is the normal refresh path after installing a new mount
  binary:

```bash
sudo systemctl daemon-reload
sudo systemctl restart cortexfs.service
```

## Local Test Mount Checks

Use `tests/mounts/cortexfs` for repository integration tests, not durable state.

Before mounting:

- Confirm `tests/mounts/cortexfs` exists.
- Confirm it contains no source files, fixtures, or durable state.
- Confirm no stale mount is present:

```bash
findmnt tests/mounts/cortexfs
```

Mount a local reference tree with:

```bash
cargo run -p cortexfs --bin ctx -- bootstrap
cargo run -p cortexfs --bin cortexfs-mount -- \
  --source "$HOME/.local/share/cortexfs/root" \
  tests/mounts/cortexfs
```

After mounting, inspect the stable ABI shape:

```bash
find tests/mounts/cortexfs -maxdepth 2 -print
CTX_ROOT=tests/mounts/cortexfs cargo run -p cortexfs --bin ctx -- status
CTX_ROOT=tests/mounts/cortexfs cargo run -p cortexfs --bin ctx -- ls
CTX_ROOT=tests/mounts/cortexfs cargo run -p cortexfs --bin ctx -- ls home
CTX_ROOT=tests/mounts/cortexfs cargo run -p cortexfs --bin ctx -- doctor
```

After code changes, rebuild and remount. Do not expect a mounted instance to
hot-update.

## File ABI Checks

Prefer `ctx` for ABI smoke tests:

```bash
ctx file classify tool/fs.read
ctx file cat agent/coder.d/policy
ctx file check agent/coder.d/mount
ctx which tool fs.read
ctx path shared project-a
```

`ctx ls` is path-based:

```bash
ctx ls
ctx ls /
ctx ls home
ctx ls model
ctx ls agent
ctx ls tool
ctx ls shared/project-a
```

For `model`, `agent`, and `tool`, `ctx ls` may filter to visible object names
instead of showing `.sock` and `.d` implementation entries. For arbitrary ABI
paths such as `home` or `shared/project-a`, it should list raw directory names.

## Socket And agent.sh Checks

`agent.sh` is a thin client over `/ctx/agent/<agent>.sock`; it is not the
CortexFS runtime and must not start listeners itself.

Interpret socket failures carefully:

- `Permission denied` from `nc -U /ctx/agent/coder.sock` means Linux mode bits,
  ownership, FUSE `default_permissions`, or mount options blocked `connect(2)`
  before the runtime saw the request.
- `Connection refused` means the socket path exists but no process is
  listening.
- A JSONL `{"type":"error","code":"EACCES",...}` frame means socket
  connected runtime/policy refused operation.

Useful probes:

```bash
stat -c '%A %a %U %G %n' /ctx /ctx/agent /ctx/agent/coder.sock
printf '{"op":"ping"}\n' | nc -U /ctx/agent/coder.sock
./agent.sh/agent.sh --status coder
./agent.sh/agent.sh --raw --session default coder 'ping'
```

Do not treat reference-tree socket inode as proof runtime listening.
Reference trees can expose socket paths as ABI shape; live conversation
requires a real listener.

## Live Agent Execution Proof

A static `tools` list proves discovery only; it does not prove the agent can
execute tools, see workspace, or reach configured backend. Before claiming live
ctx-agent test succeeded:

1. Verify a real FUSE mount with `ctx status` plus `ctx doctor`.
2. Probe canonical role path `ctx ping agent/ROLE`. Require exactly one JSON
   `pong`, then require the first line of `ctx agent status ROLE` to be exactly
   `idle`.
3. Run a harmless tool through same agent/session and verify result in
   trajectory events. Include a known workspace file in the probe so result
   proves task workspace is actually visible.
4. Capture the relevant process-tree chain (`systemd` unit and PIDs). Use it to
   prove runtime-model backend handling; model response text alone is insufficient.

If the task explicitly requires ctx-agent execution, do not fall back to host CLI
like `codex`, `claude`, or other host-side agent CLIs. Stop and report failed
proof instead.

Obtain explicit authorization before real provider call, paid task, external job,
or upload.

Classify residue before cleanup; never delete by name through `/ctx`; retain
report for every identity mismatch:

- Session directory: use session GC preview and act only on exact selection.
- Receipt-managed installed object: use object residue audit/cleanup; recheck its
  receipt backing-generation `dev`/`ino` immediately before acting.
- Incomplete agent control fragment owned by neither: retain report unless exact
  cleanup procedure defines ownership.

## AUR Package Checks

The package name is `cortexfs-git`.

After main repository changes are pushed, update the local AUR checkout when
requested:

```bash
cd /tmp/cortexfs-git-aur
makepkg -o --noconfirm
makepkg --printsrcinfo > .SRCINFO
git diff -- PKGBUILD .SRCINFO
```

Install locally with:

```bash
makepkg -si --noconfirm
pacman -Q cortexfs-git
```

The AUR package should install:

```text
/usr/bin/ctx
/usr/bin/cortexfs-mount
/usr/lib/systemd/system/cortexfs.service
```

If `git push aur` fails with SSH/public-key errors, report the exact failure and
leave the local AUR commit intact. Do not claim the AUR remote was updated.

## README SVG Benchmark Checks

README visuals and benchmark SVGs are generated by:

```bash
scripts/update-readme-svg.sh
xmllint --noout docs/assets/*.svg
```

The benchmark script should avoid invalid commands such as `ctx ls /` only if
the installed `ctx` does not support path-based `ls`. Current `ctx` supports
`ctx ls`, `ctx ls /`, and `ctx ls home`.

## Live Model Checks

Use local Ollama only as the current live-test fixture:

1. Check whether the Ollama daemon is reachable on `127.0.0.1:11434`.
2. Check whether `smollm2:135m` appears in `ollama list`.
3. If missing, report that the model must be installed or pulled. Do not
   silently switch to another model.
4. Use short prompts and deterministic expectations for smoke tests.

Example minimal prompt:

```text
Reply with exactly: cortexfs-ok
```

If the user explicitly asks to test their configured provider or local
aggregation API, use the existing provider registry, route, secret state, and
unified submission semantics. Do not hard-code that provider into core defaults
or special branches.

## Reporting

Report:

- commands run,
- whether `/ctx` or `tests/mounts/cortexfs` was used,
- whether the systemd service was enabled/active when relevant,
- whether `allow_other` was present when testing non-root access,
- whether a real runtime socket listener existed,
- which canonical ping/status probes passed,
- which real tool smoke proved execution and workspace visibility,
- which process tree proved the runtime/model backend,
- whether Ollama was reachable,
- whether `smollm2:135m` was available,
- whether a real FUSE mount was attempted,
- whether AUR was installed, committed, or pushed,
- whether cleanup was previewed and identity-bound,
- any skipped step and the concrete reason.
