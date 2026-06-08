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

创建推荐挂载点：

```bash
sudo mkdir -p /ctx
sudo chown "$USER:$USER" /ctx
```

启动挂载：

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
fusermount3 -u /ctx
```

## systemd 后台挂载

AUR 包安装 systemd 模板服务 `cortexfs@.service`。`/ctx` 是系统路径，所以服务由 systemd 管理挂载点，再把实际 FUSE 进程降权到指定用户运行。

启用当前用户的后台挂载：

```bash
sudo systemctl enable --now "cortexfs@$USER.service"
```

查看状态：

```bash
systemctl status "cortexfs@$USER.service"
findmnt /ctx
cat /ctx/status
```

停止并卸载：

```bash
sudo systemctl stop "cortexfs@$USER.service"
```

如果你手动杀掉了 `cortex mount`，可能会留下坏 FUSE endpoint。先清理再重启服务：

```bash
fusermount3 -u /ctx
sudo systemctl restart "cortexfs@$USER.service"
```

## 多用户部署

多用户挂载需要显式开启 CortexFS 的 multi-user 模式，并允许 FUSE 使用 `allow_other`。

1. 编辑 `/etc/fuse.conf`，启用这一行：

```text
user_allow_other
```

2. 准备挂载点权限，让需要访问的本机用户可以进入 `/ctx`：

```bash
sudo mkdir -p /ctx
sudo chmod 755 /ctx
```

3. 使用已安装的 `cortex` 命令挂载：

```bash
cortex mount --multi-user /ctx
```

路径只是命名空间，不是安全边界。多用户部署仍然应该依赖 host credential、external subject、object context 和 Cortex policy 做实际访问决策。

## 从源码构建

源码构建适合开发者和 CI，不是用户文档里的默认安装方式：

```bash
cargo build --locked --workspace
cargo run -p cortex-cli -- status
```

仓库内 FUSE 集成测试固定使用 `tests/mounts/cortexfs`，不要在该目录放源码、fixture 或持久化数据。
