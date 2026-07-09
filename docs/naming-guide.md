# CortexFS Naming Guide

This guide is the durable project convention for files, modules, and functions
under `crates/cortexfs/src`. It aligns multi-word filenames with kebab-case on
disk, Rust module identifiers with `snake_case`, and helpers with short
kernel-style verbs.

Contributor entry points that point here: [AGENTS.md](https://github.com/LIghtJUNction/cortexfs/blob/main/AGENTS.md),
[developing-cortexfs.md](developing-cortexfs.md).

## 1. File Naming Rules

- **Case**: Filenames are strictly lowercase.
- **Word separation**: Multi-word Rust sources use dashes (`-`), not underscores
  (`_`). Prefer `plain-fs.rs` over `plain_fs.rs`.
- **Extension**: Rust sources use `.rs`.
- **No `mod.rs`**: Never introduce `mod.rs`. A directory module is declared from
  a sibling file (`agent.rs` + `agent/…`) or an explicit `#[path]` target.
- **Test suffixes**: Co-located tests use a `-tests.rs` suffix
  (e.g. `permission-tests.rs`).
- **Helpers**: Name helper modules after their primary utility
  (e.g. `process-helpers.rs`).
- **Scope**: These file rules apply to production and bin sources under
  `crates/cortexfs/src`. The unit tree under `crates/cortexfs/tests/unit/**` may
  still use snake_case paths with `include!` until a separate rename.

## 2. Module Naming Rules

- **Identifiers**: Module names in Rust remain `snake_case`
  (`pub mod plain_fs`).
- **Disk mapping**: Multi-word kebab files must be declared with `#[path]`:

  ```rust
  #[path = "plain-fs.rs"]
  pub mod plain_fs;
  ```

- **Directory modules**: Prefer a parent file (`support.rs`) that lists children
  with `#[path = "support/…"]` rather than a `mod.rs` inside the directory.
- **Public roots**: When a module moves under a grouping root (`support`,
  `fuse`, `runtime`, …), keep existing `crate::…` call sites working via
  re-exports from `lib.rs` / `exports.rs` when those paths were already public.

## 3. Function Naming Rules

- **Case**: Functions and methods use `snake_case`.
- **Kernel-style verbs**: Prefer short, standard action prefixes:

  | Prefix | Use |
  | --- | --- |
  | `new_` / `alloc_` | Construct or allocate |
  | `destroy_` / `free_` | Tear down or release ownership |
  | `get_` / `put_` | Acquire or release a reference/resource |
  | `read_` / `write_` | Stream or filesystem I/O |
  | `parse_` / `format_` | Serialization / text shapes |
  | `is_` / `has_` | Boolean predicates |

- **Conciseness**: Prefer a short verb phrase over a long sentence-name.
- **Scope of renames**: Do not mass-rename the whole tree for style alone. Rename
  when moving a symbol with a module, finishing a started rename, or fixing a
  dual old/new helper.

## 4. Shared quality models

Issue/report shapes that used to be copy-pasted per domain now share bases under
`support/`:

| Domain aliases | Shared base |
| --- | --- |
| `AgentControlIssue`, `SessionControlIssue`, `SessionIndexIssue`, `ToolSchemaIssue` | `ControlLineIssue` (`support/control-text.rs`) |
| `ObjectLayoutIssue`, `SessionLayoutIssue`, `SharedQueueLayoutIssue` | `PathLayoutIssue` + `LayoutPathRole` (`support/layout-path.rs`) |

Prefer `inspect_control_line` / `inspect_control_lines`, `for_each_jsonl_line` /
`parse_jsonl_line`, and `require_plain_*` / `create_plain_dir_with` over new
local copies. Domain crates may keep **type aliases** for readability; do not
reintroduce parallel `EmptyValue` / `MissingFile` / `InvalidJson` enums for
those families.

## 5. Checklist for new code

1. New multi-word file → kebab-case name + `#[path]` in the parent module.
2. No new `mod.rs`.
3. No second copy of a helper under a snake_case path while a kebab path exists.
4. Public call sites still compile after a move (re-export or update imports).
5. Prefer existing helpers (`plain_fs`, `host_path`, `process_helpers`,
   `ControlLineIssue`, `PathLayoutIssue`, `jsonl_line`, `bin/shared/*`,
   provider/route/secret paths) before inventing a synonym.
6. New control/layout validation issues map onto the shared bases (or a thin
   alias), not a fresh parallel enum.
