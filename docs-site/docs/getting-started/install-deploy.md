---
title: 安装与部署
---

# 安装与部署

CortexFS 面向终端用户的入口是已安装的 `cortex` 命令。Arch Linux 用户可以直接安装 AUR 包 `cortexfs-git`，不需要在日常使用时通过 `cargo run` 启动。

## Arch Linux

使用任意 AUR helper 安装：

```bash
paru -S cortexfs-git
```

安装完成后确认 CLI 可用：

```bash
cortex status
```

`status` 会输出推荐挂载点 `/ctx`、当前 ABI 名称、默认测试挂载点和 live-test fixture 信息。

## 单用户部署

推荐使用 systemd 后台服务启动挂载：

```bash
cortex start
```

`cortex start` 本质上执行 systemd 服务 `cortexfs@$USER.service`。CLI 会在需要时调用 `sudo systemctl`，服务会自动加载 FUSE、清理坏挂载、创建 `/ctx` 并设置 owner/mode。默认部署不需要手动创建 `/ctx`，也不需要手动配置挂载权限。

如果需要临时前台调试，也可以手动挂载：

```bash
cortex mount /ctx
```

`cortex mount` 是前台进程。保持该终端运行，然后在另一个终端读取挂载树：

```bash
export CTX_HOME="/ctx/home/$(id -u)"
cat /ctx/status
cat /ctx/provider/list
cat "$CTX_HOME/model/list"
```

卸载：

```bash
cortex stop
```

## systemd 后台挂载

AUR 包安装 systemd 模板服务 `cortexfs@.service`。`/ctx` 是系统路径，所以服务由 systemd 管理挂载点，再把实际 FUSE 进程降权到指定用户运行。

启用当前用户的后台挂载：

```bash
cortex start
```

查看状态：

```bash
systemctl status "cortexfs@$USER.service"
findmnt /ctx
cat /ctx/status
```

停止并卸载：

```bash
cortex stop
```

如果你手动杀掉了前台 `cortex mount`，可能会留下坏 FUSE endpoint。后台服务启动前会自动清理它：

```bash
cortex restart
```

## 多用户部署

默认的 `cortex start` 是单用户挂载：`/ctx` 由 systemd 准备，FUSE 进程以当前用户运行。需要跨 Linux 用户共享同一个挂载时，才使用显式 multi-user 模式：

```bash
cortex mount --multi-user /ctx
```

multi-user 是高级部署形态；路径只是命名空间，不是安全边界。多用户部署仍然应该依赖 host credential、external subject、object context 和 Cortex policy 做实际访问决策。

## 从源码构建

源码构建适合开发者和 CI，不是用户文档里的默认安装方式：

```bash
cargo build --locked --workspace
cargo run -p cortex-cli -- status
```

仓库内 FUSE 集成测试固定使用 `tests/mounts/cortexfs`，不要在该目录放源码、fixture 或持久化数据。
