# CortexFS Naming Guide

Durable convention for files, modules, and functions under
`crates/cortexfs/src`. Taste target: **Linux kernel source style** — short,
lowercase, single-token module stems; no ornamental separators.

Contributor entry points:
[AGENTS.md](https://github.com/LIghtJUNction/cortexfs/blob/main/AGENTS.md),
[DESIGN.md](DESIGN.md), [developing-cortexfs.md](developing-cortexfs.md).

## 1. File naming

- **Case**: lowercase only.
- **Module stem**: prefer a **single token** (`rules.rs`, `skills.rs`,
  `snapshot.rs`, `history.rs`). **Do not** introduce new module filenames with
  `-` or `_` in the stem.
- **Compound ideas**: pick a stronger noun, not a phrase. Prefer `snapshot`
  over `load-snapshot` / `load_snapshot`.
- **Extension**: `.rs`.
- **No `mod.rs`**: never. Directory modules are declared from a sibling file
  (`agent.rs` + `agent/…`). Rename legacy files instead of retaining an
  explicit `#[path]` declaration.
- **Tests**: co-located production-adjacent tests may use a `tests` suffix
  only when already established; prefer `tests/unit/**` for new coverage.
- **Scope**: production and bin sources under `crates/cortexfs/src`. The unit
  tree under `crates/cortexfs/tests/unit/**` may still use older path habits
  until a separate rename.

### Canonical module names

Every module has exactly one canonical name matching its file stem. When a
module moves or is renamed, update all call sites in the same change. Do not
retain a second module path through `pub use new as old` or an equivalent
compatibility alias.

Do not add `#[path]` or test `include!` module workarounds. Do not add production
glob imports, caller/domain-prefixed aliases for shared
helpers (`use helper as caller_helper`), or thin wrappers that only rename or
pass through another function. Generated `include!` from `OUT_DIR`, aliases
needed to disambiguate traits or colliding types, the frozen test-parent globs
inside `src/tests/**` and `object/executor/tests/**`, and the existing
`tests/unit/ctx*` flat harness are the only exceptions. These exceptions retain
shared fixtures, stable subprocess test names, and shared lexical scope; do not
extend their glob or `include!` surface.

## 2. Module identifiers

- Rust module names follow the file stem when the stem is a single token:
  `pub mod snapshot;` → `snapshot.rs`.
- Multi-word **Rust identifiers** remain `snake_case` (`write_run_snapshot`).
  That is Rust syntax, not an excuse for `load_snapshot.rs` on disk.
- Prefer a parent file (`support.rs`, `prompt.rs`) that lists children over a
  `mod.rs` inside the directory.
- Public roots follow the same rule: update callers to the canonical path
  instead of preserving an old root module name through a re-export alias.

## 3. Function naming

- **Case**: `snake_case` (Rust).
- **Length**: short verb + object. Prefer `write_snapshot` over
  `write_agent_load_snapshots_for_run`.
- **Kernel-style verbs**:

  | Prefix | Use |
  | --- | --- |
  | `new_` / `alloc_` | construct or allocate |
  | `destroy_` / `free_` | tear down |
  | `get_` / `put_` | acquire or release a reference |
  | `read_` / `write_` | I/O |
  | `parse_` / `format_` | text shapes |
  | `is_` / `has_` | predicates |

- **Rename scope**: rename when moving a symbol, finishing a started rename, or
  fixing a dual old/new helper—not for fashion across the whole tree.

## 4. Shared quality models

Issue/report shapes share bases under `support/`:

| Domain aliases | Shared base |
| --- | --- |
| `AgentControlIssue`, `SessionControlIssue`, `SessionIndexIssue`, `ToolSchemaIssue` | `ControlLineIssue` (`support/control.rs`) |
| `ObjectLayoutIssue`, `SessionLayoutIssue`, `SharedQueueLayoutIssue` | `PathLayoutIssue` + `LayoutPathRole` (`support/layout.rs`) |

Prefer `inspect_control_line` / `inspect_control_lines`, `for_each_jsonl_line` /
`parse_jsonl_line`, `require_plain` (or `require_symlink_dir` for symlink-metadata
dirs), and `create_plain_dir_with` over new local copies. Domain type aliases
are fine; parallel `EmptyValue` / `MissingFile` / `InvalidJson` enums are not.

## 5. Checklist for new code

1. New module file → **single lowercase token**, no `-`, no `_` in the stem.
2. No new `mod.rs`.
3. No second helper that only renames or re-wraps an existing one.
4. All call sites use the one canonical module path after a move; do not add a
   compatibility module alias.
5. Prefer existing helpers (`support::plain`, `support::path`,
   `support::process`, `support::control`, `support::layout`,
   `support::jsonl`, `cli/*`, provider/route/secret paths).
6. New control/layout issues map onto shared bases (or a thin alias).
7. Names should look at home next to kernel-style short modules (`rules`,
   `skills`, `snapshot`), not framework-style phrases.
8. No new `#[path]`, test `include!` module workaround, compatibility module alias,
   caller-prefixed shared-helper alias, new glob import, or rename/pass-through
   wrapper (except generated `OUT_DIR` includes, trait/type disambiguation,
   frozen parent globs in the migrated test trees, and the frozen legacy
   `tests/unit/ctx*` flat harness).
