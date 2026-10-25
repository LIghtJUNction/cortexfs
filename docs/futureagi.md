# Evaluation adapter removed

The `cortexfs-futureagi` executable is no longer built or shipped. Evaluation,
vendor-specific dataset conversion, evaluator discovery, and cloud submission
are outside CortexFS's filesystem mapping and interface scope.

Existing automation invoking `cortexfs-futureagi export` or `evaluate` must move
to an external evaluation tool. There is no replacement CortexFS evaluator.
The existing `ctx agent trajectory` command and ATIF representation remain
unchanged in this removal; consumers can read that interface and perform their
own conversion outside CortexFS.

This source change does not delete stored trajectories, sessions, exported
files, credentials, or an old manually installed executable. No mount, root
path, permission, socket, or file-operation contract changes.
