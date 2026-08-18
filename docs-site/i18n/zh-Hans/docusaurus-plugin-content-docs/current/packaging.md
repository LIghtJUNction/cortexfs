---
id: packaging
title: Linux 安装包
sidebar_label: Linux 安装包
---

# Linux 安装包

CortexFS 为源码安装器支持的 Linux 发行版提供原生包元数据：Arch、Debian/Ubuntu、
Fedora/RHEL 与 openSUSE/SLES。安装包包含发布二进制文件、systemd 单元、运行目录、MIT
许可证以及规范文档。
发布产物同时包含 `cortexfs-channel` 以支持多平台 IM bridge；安装后需按
[通道文档](channels.md) 配置。

使用宿主机原生打包工具从检出目录构建安装包：

```bash
./packaging/build.sh --format deb
./packaging/build.sh --format rpm
./packaging/build.sh --format arch
./packaging/build.sh --format tar
```

`--format all` 会在当前主机上构建全部可用格式。Debian、Arch 与 tar 格式会执行一次
统一构建。RPM 使用原生 spec 并从源码归档重建，这样才能满足 Fedora/RHEL 的构建服务
路径。

使用系统原生包管理器安装生成的包：

```bash
# Debian 或 Ubuntu
sudo apt install ./dist/cortexfs_0.1.7_amd64.deb

# Fedora、RHEL 兼容系统，或具有 RPM 工具链的 openSUSE/SLES
sudo dnf install ./dist/cortexfs-0.1.7-*.rpm

# Arch Linux
sudo pacman -U ./dist/cortexfs-0.1.7-1-x86_64.pkg.tar.zst
```

tarball 是面向无原生包管理器环境的可移植文件系统负载；请先确认内容后仅解压到 `/`：

```bash
tar -tzf dist/cortexfs-0.1.7-linux-x86_64.tar.gz
sudo tar -C / -xzf dist/cortexfs-0.1.7-linux-x86_64.tar.gz
```

默认输出目录是 `dist/`。安装器会创建配置与存储父目录，但在卸载时不删除它们。
首次安装会启用 `cortexfs.service` 但不启动；请在确认主机的 FUSE 与 bubblewrap 设置
后再启动：

```bash
sudo systemctl start cortexfs.service
ctx doctor
```

安装包不会创建新的 `/ctx` ABI 入口或第二套配置存储。agent socket 仍是现有的
`cortexfs-agent@.socket` 实例，并且包升级只在新文件安装完成后重启已运行的挂载服务。

对于仅源代码部署且缺少原生打包构建环境，仍可使用现有安装脚本：

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
```
