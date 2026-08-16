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

Install the resulting package with the native package manager:

```bash
# Debian or Ubuntu
sudo apt install ./dist/cortexfs_0.1.7_amd64.deb

# Fedora, RHEL-compatible systems, or openSUSE/SLES with RPM tooling
sudo dnf install ./dist/cortexfs-0.1.7-*.rpm

# Arch Linux
sudo pacman -U ./dist/cortexfs-0.1.7-1-x86_64.pkg.tar.zst
```

The tarball is a portable filesystem payload for environments without a native
package manager; extract it at `/` only after reviewing its contents:

```bash
tar -tzf dist/cortexfs-0.1.7-linux-x86_64.tar.gz
sudo tar -C / -xzf dist/cortexfs-0.1.7-linux-x86_64.tar.gz
```

The output directory is `dist/` by default. The package installer creates the
configuration and storage parents but does not delete them on removal. It
enables `cortexfs.service` without starting it on a first install; start it
after reviewing the host's FUSE and bubblewrap setup:

```bash
sudo systemctl start cortexfs.service
ctx doctor
```

The package does not create a new `/ctx` ABI entry or a second configuration
store. Agent sockets remain the existing `cortexfs-agent@.socket` instances,
and package upgrades restart an already active mount service only after the new
files have been installed.

For a source-only deployment with no native package builder, the existing
installer remains available:

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
```
