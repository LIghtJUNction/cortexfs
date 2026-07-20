# Executable Object Manifests

Use v2 for a new host-side manifest:

```json
{
  "schema": "cortexfs.object/v2",
  "version": "0.1.0",
  "compatibility": {
    "cortexfs": ">=0.1.7, <0.2.0"
  },
  "class": "tool",
  "name": "project.echo",
  "executable": {
    "path": "target/release/project-echo",
    "sha256": "64 lowercase or uppercase hexadecimal characters"
  },
  "controls": {}
}
```

`version` must be a SemVer. `compatibility.cortexfs` must be a Cargo-style
SemVer requirement. `ctx object check` and `ctx object install` match it
against the CortexFS package version compiled into the current `ctx`. A
mismatch is invalid input, exits 2, and performs no writes.

The legacy v1 shape is strict: use `schema: cortexfs.object/v1` and omit both
`version` and `compatibility`. Supplying either field to v1 is invalid; no
cross-version inference or fallback occurs.

Resolve a relative executable path against the manifest directory. Keep the
install tier outside the manifest and pass it explicitly to the CLI.

Reject unknown top-level fields, unknown `executable` fields, unknown controls,
symbolic links, non-regular or non-executable artifacts, digest mismatches, and
control characters. Do not accept `command`, `args`, or wrapper fragments.

## Tool controls

Supply all of `description`, `schema`, `cap`, and `policy`. Derive `name` from
the manifest object name. Do not supply runtime-owned `status` or `log`; the
installer initializes them canonically.

Choose `--tier user` for `/ctx/home/<effective-uid>/tool` or `--tier system`
for `/ctx/tool`. Installation creates the object but grants no agent access.

## Agent controls

Supply explicit controls: `owner`, `uid`, `gid`, `groups`, `label`, `iso`,
`parent`, `life`, `root`, `cwd`, `env`, `path`, `mount`, `model`, `abi`, and
`policy`. Set `abi` to exactly `sdk-envelope-v1`. Optionally supply `approval`,
`system.md`, `prompt.template.md`, `meta.json`, and `tools`. `tools` is a
newline-terminated, one-name-per-line static direct-native
set; every name still needs explicit matching agent- and tool-policy grants.
The installer never synthesizes grants or writes tsh/session cache state.
Do not supply runtime-owned `status`, `pid`, `log`, or socket state. The
installer initializes the three runtime control files but never creates a
socket.

Install agents with `--tier system`. The root ABI still defines user agents at
`home/<uid>/agent`, but neither manifest schema carries tier identity into the
root socket runtime, so the installer rejects `agent --tier user`.

## Publication semantics

Require the object name to be absent. Stage and sync the complete control
directory and executable in the destination filesystem. Publish the control
directory with no-replace semantics, then publish the executable last with
no-replace semantics, and verify both receipts again before reporting success.
Success or failure may retain a hidden `.cortexfs-install-*` safety residue;
leave it for explicit future cleanup. Preserve an existing object byte-for-byte
on collision.

A `cortexfs.object-install/v2` receipt records `object_version` and
`cortexfs_requirement`; a `cortexfs.object-install/v1` receipt records neither.
Inspection exposes these compatibility facts. They do not grant authority or
start a runtime, and a later CortexFS mismatch does not prevent
receipt-managed uninstall.

Treat a multi-object extension as an ordered sequence of single-object
installs. Do not claim bundle atomicity. Installation is new-object-only.

## Receipt-managed replacement

Replacement candidates always use `cortexfs.object/v2` and name the exact
installed class/name:

```text
ctx object replace --source PATH MANIFEST [--tier user|system] [--yes]
ctx object upgrade --source PATH MANIFEST [--tier user|system] [--yes]
ctx object rollback --source PATH MANIFEST [--tier user|system] [--yes]
```

All three commands default to dry-run. `replace` accepts a receipt-managed v1
or v2 current object without ordering its versions. `upgrade` requires current
v2 and a strictly higher candidate. `rollback` requires current v2 and a
strictly lower candidate; CortexFS stores no version history, so the caller
supplies the old manifest and exact artifact. `--yes` is the explicit mutation
boundary.

Applied replacement prepares and syncs the candidate in a same-filesystem
stage, hides the old executable first, and publishes the new executable last.
Before that commit boundary, failure restores the exact old pair when safe.
Receipt conflicts do not intentionally overwrite or delete foreign inodes and
may retain audit-visible safety residue. This is not pair atomicity.

Quiesce the matching runtime and same-authority writers before `--yes`.
Replacement does not retain version history, stop or start a runtime, grant
policy authority, or create socket state.
