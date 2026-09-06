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
可选打包适配器还包括官方 Agent SDK 角色二进制
（`cortexfs-agent-architect`、`cortexfs-agent-executor`、
`cortexfs-agent-product-manager`）以及一次性
[`cortexfs-futureagi`](futureagi.md) 评估器。它们不是额外的 `/ctx` 根。
源码安装器与 RPM spec 都会构建 `-p cortexfs-agents`，与 Debian、Arch、tar
负载一致。

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

使用系统原生包管理器安装生成的包。文件名使用 `Cargo.toml` 中的 workspace 版本
（当前为 `0.1.21`）：

```bash
# Debian 或 Ubuntu
sudo apt install ./dist/cortexfs_*_amd64.deb

# Fedora、RHEL 兼容系统，或具有 RPM 工具链的 openSUSE/SLES
sudo dnf install ./dist/cortexfs-*-*.rpm

# Arch Linux
sudo pacman -U ./dist/cortexfs-*-x86_64.pkg.tar.zst
```

tarball 是面向无原生包管理器环境的可移植文件系统负载；请先确认内容后仅解压到 `/`：

```bash
tar -tzf dist/cortexfs-*-linux-x86_64.tar.gz
sudo tar -C / -xzf dist/cortexfs-*-linux-x86_64.tar.gz
```

默认输出目录是 `dist/`。安装器会创建配置与存储父目录，但在卸载时不删除它们。
首次安装会启用 `cortexfs.service` 和 `cortexfs-terminal-broker.socket` 但不启动挂载；
请在确认主机的 FUSE 与 bubblewrap 设置后再启动：

```bash
sudo systemctl start cortexfs.service
ctx doctor
```

安装包不会创建新的 `/ctx` ABI 入口或第二套配置存储。agent 控制 socket 仍是现有的
`cortexfs-agent@.socket` 实例。终端会话使用 root 所有、socket 激活的 broker，
包升级只在新文件安装完成后重启已运行的挂载服务。

打包的 systemd 单元钉死硬 cgroup 上限，避免单个 agent 耗尽主机。
`cortexfs.service` 使用 `MemoryMax=512M`、`CPUQuota=100%`、`TasksMax=64`。
`cortexfs-agent@.service` 使用 `MemoryMax=512M`、`CPUQuota=100%`、`TasksMax=128`。
单元设置 `MemoryAccounting=` 与 `TasksAccounting=`；不单独写 `CPUAccounting=`，
因为设置 `CPUQuota=` 时 systemd 已经打开 CPU accounting。交互式
`ctx agent start` 终端不同：`systemd-run --user` 仍通过 `support::quota` 传入
`CPUAccounting=yes`，以及 `MemoryMax=1G`、`CPUQuota=200%`、`TasksMax=256`。

首次安装之后，主机更新使用同一套原生包后端：

```bash
ctx update --ref main          # 解析并检查不可变计划
ctx update --ref main --yes    # 构建、安装、验证或回滚
```

成功的 ref 更新会记录跟踪 ref，之后的计划可以省略 `--ref`。本地钉死提交用
`--source /clean/git/checkout`。更新器保留配置、存储、密钥和未活动单元状态；
包所有权安装仅在精确的当前包可用于回滚时才会应用。
活动单元发现会匹配精确的 `cortexfs.service` 挂载单元，以及
`cortexfs-agent@executor.service` 这类带连字符的实例名。事务前未活动的单元保持未活动。

`ctx update` 必须以普通用户运行，并具备 `sudo`、`flock` 和控制终端（`/dev/tty`）。
它拒绝以 root 运行。已安装 helper 是 `/usr/lib/cortexfs/update-linux`；当该路径就是
正在运行的脚本时，它必须是 root 所有、非符号链接、且不可被组/其他人写。
apply 会从钉死检出中 source `scripts/install-linux.sh`，因此 helper 不需要第二份
打包的安装器副本。预检仍要求 FUSE 与 bubblewrap 0.10+ 才开始构建。

对于仅源代码部署且缺少原生打包构建环境，仍可使用现有安装脚本：

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
```
