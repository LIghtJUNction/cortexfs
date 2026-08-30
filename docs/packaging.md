---
id: packaging
title: Linux Packages
sidebar_label: Linux Packages
---

# Linux Packages

CortexFS provides native package metadata for the Linux families supported by
the source installer: Arch, Debian/Ubuntu, Fedora/RHEL, and openSUSE/SLES.
Packages contain the release binaries, systemd units, runtime directories, the
MIT license, and the normative specification files.
The release payload also includes `cortexfs-channel` for the multi-IM bridge;
configure it through the [channel guide](channels.md) after installation.
Optional packaged adapters include the official Agent SDK role binaries
(`cortexfs-agent-architect`, `cortexfs-agent-executor`,
`cortexfs-agent-product-manager`) and the one-shot
[`cortexfs-futureagi`](futureagi.md) evaluator. Those binaries are not extra
`/ctx` roots.

Build packages from a checkout with the host's native package tools:

```bash
./packaging/build.sh --format deb
./packaging/build.sh --format rpm
./packaging/build.sh --format arch
./packaging/build.sh --format tar
```

`--format all` builds every format available on the host. A release build is
performed once for the Debian, Arch, and tar formats. RPM builds use the native
spec and rebuild from the source archive, which keeps the RPM path suitable for
Fedora/RHEL build services.

Install the resulting package with the native package manager. Filenames use
the workspace version from `Cargo.toml` (currently `0.1.20`):

```bash
# Debian or Ubuntu
sudo apt install ./dist/cortexfs_*_amd64.deb

# Fedora, RHEL-compatible systems, or openSUSE/SLES with RPM tooling
sudo dnf install ./dist/cortexfs-*-*.rpm

# Arch Linux
sudo pacman -U ./dist/cortexfs-*-x86_64.pkg.tar.zst
```

The tarball is a portable filesystem payload for environments without a native
package manager; extract it at `/` only after reviewing its contents:

```bash
tar -tzf dist/cortexfs-*-linux-x86_64.tar.gz
sudo tar -C / -xzf dist/cortexfs-*-linux-x86_64.tar.gz
```

The output directory is `dist/` by default. The package installer creates the
configuration and storage parents but does not delete them on removal. It
enables `cortexfs.service` and `cortexfs-terminal-broker.socket` without
starting the mount on a first install; start it after reviewing the host's
FUSE and bubblewrap setup:

```bash
sudo systemctl start cortexfs.service
ctx doctor
```

The package does not create a new `/ctx` ABI entry or a second configuration
store. Agent control sockets remain the existing `cortexfs-agent@.socket`
instances. Terminal sessions use the root-owned socket-activated broker, and
package upgrades restart an already active mount service only after the new
files have been installed.

Packaged systemd units pin hard cgroup ceilings so one agent cannot exhaust the
host. `cortexfs.service` uses `MemoryMax=512M`, `CPUQuota=100%`, and
`TasksMax=64`. `cortexfs-agent@.service` uses `MemoryMax=512M`, `CPUQuota=100%`,
and `TasksMax=128`. The units set `MemoryAccounting=` and `TasksAccounting=`;
they omit a separate `CPUAccounting=` line because `CPUQuota=` already turns
CPU accounting on. Interactive `ctx agent start` terminals are different:
`systemd-run --user` still passes `CPUAccounting=yes` through `support::quota`
along with `MemoryMax=1G`, `CPUQuota=200%`, and `TasksMax=256`.

After the first installation, host updates use the same native package backend:

```bash
ctx update --ref main          # resolve and inspect the immutable plan
ctx update --ref main --yes    # build, install, verify, or roll back
```

A successful ref update records its tracking ref, so later plans may use
`ctx update` without repeating `--ref`. Use `--source /clean/git/checkout` for
a pinned local commit. The updater preserves configuration, storage, secrets,
and inactive unit state; a package-owned install applies only when its exact
current package is available for rollback.

`ctx update` must run as a normal user with `sudo`, `flock`, and a controlling
terminal (`/dev/tty`). It refuses to run as root. The installed helper is
`/usr/lib/cortexfs/update-linux`; when that path is the running script it must
be a root-owned, non-symlink, non-group-writable file. Apply then sources
`scripts/install-linux.sh` from the pinned checkout, so the helper does not
need a second packaged copy of the installer. Preflight still requires FUSE
and bubblewrap 0.10+ before building.

For a source-only deployment with no native package builder, the existing
installer remains available:

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
```
