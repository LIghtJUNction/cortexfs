# Executable Object Manifest v1

Use this host-side manifest shape:

```json
{
  "schema": "cortexfs.object/v1",
  "class": "tool",
  "name": "project.echo",
  "executable": {
    "path": "target/release/project-echo",
    "sha256": "64 lowercase or uppercase hexadecimal characters"
  },
  "controls": {}
}
```

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

Supply explicit authority controls: `owner`, `uid`, `gid`, `groups`, `label`,
`iso`, `parent`, `life`, `root`, `cwd`, `env`, `path`, `mount`, `model`, and
`policy`. Optionally supply `system.md`, `prompt.template.md`, `meta.json`, and
`tools`. `tools` is a newline-terminated, one-name-per-line static direct-native
set; every name still needs explicit matching agent- and tool-policy grants.
The installer never synthesizes grants or writes tsh/session cache state.
Optionally supply `abi` with exactly `argv-v1` or `sdk-envelope-v1`. Omitting
`abi` means the exact legacy `argv-v1` launch contract; SDK envelope agents must
opt in with `sdk-envelope-v1`.
Do not supply runtime-owned `status`, `pid`, `log`, or socket state. The
installer initializes the three runtime control files but never creates a
socket.

Install agents with `--tier system`. The root ABI still defines user agents at
`home/<uid>/agent`, but `cortexfs.object/v1` cannot carry tier identity into the
root socket runtime, so the v1 installer rejects `agent --tier user`.

## Publication semantics

Require the object name to be absent. Stage and sync the complete control
directory and executable in the destination filesystem. Publish the control
directory with no-replace semantics, then publish the executable last with
no-replace semantics, and verify both receipts again before reporting success.
Success or failure may retain a hidden `.cortexfs-install-*` safety residue;
leave it for explicit future cleanup. Preserve an existing object byte-for-byte
on collision.

Treat a multi-object extension as an ordered sequence of single-object
installs. Do not claim bundle atomicity.
