# CortexFS repository instructions

Read [PROJECT_SCOPE.md](PROJECT_SCOPE.md) first. It is the active authority for this product reset.

## Current state

- No new product direction has been approved.
- This checkout contains historical documentation and recipes, not buildable product source.
- The former Agent runtime, FUSE /ctx system, model/session stack, channels, process sandbox, and MCP infrastructure are retired from the active scope.
- Files under docs/ and recipes/ are historical references. They do not create current implementation or compatibility obligations.

## Work rules

1. Do not restore old product code, add features, or revise historical ABI contracts to anticipate an unknown direction.
2. Before implementation, require one concrete user task that passes the decision gates in PROJECT_SCOPE.md.
3. Prefer an existing CLI, library, protocol, or OS primitive over a new subsystem.
4. Keep each change limited to one user-visible outcome. Avoid frameworks, registries, daemons, databases, and installers unless the approved task demonstrably requires one.
5. If an existing tool already solves the task, use it or contribute upstream instead of duplicating it.
6. Preserve historical material unless the user explicitly asks to delete it.

## Git workflow

- Work on a task branch and submit a pull request.
- Do not push directly to main, force-push, or merge a pull request unless explicitly authorized.
- Inspect the target files and current branch state before editing. Preserve unrelated work.
- Report exactly what changed and which checks actually ran. Do not claim a product build or runtime test when no implementation exists.