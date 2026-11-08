# CortexFS repository instructions

Read [PROJECT_SCOPE.md](PROJECT_SCOPE.md) first. It is the active authority for this product reset.

## Current state

- No new product direction has been approved.
- The current tree is a minimal project shell with no buildable product source.
- The former Agent runtime, FUSE /ctx system, model/session stack, channels, process sandbox, MCP infrastructure, and related build scaffolding are retired.
- Git history preserves the old documentation and reconstruction plans; they do not create current implementation or compatibility obligations.

## Work rules

1. Do not restore old product code or add features before one concrete user task passes the decision gates in PROJECT_SCOPE.md.
2. Prefer an existing CLI, library, protocol, or OS primitive over a new subsystem.
3. Keep each change limited to one user-visible outcome. Avoid frameworks, registries, daemons, databases, and installers unless the approved task demonstrably requires one.
4. If an existing tool already solves the task, use it or contribute upstream instead of duplicating it.
5. Do not recreate old documentation, ABI contracts, recipes, or agent-specific tooling without an approved need.

## Git workflow

- Work on a task branch and submit a pull request.
- Do not push directly to main, force-push, or merge a pull request unless explicitly authorized.
- Inspect the target files and current branch state before editing. Preserve unrelated work.
- Report exactly what changed and which checks actually ran. Do not claim a product build or runtime test when no implementation exists.
