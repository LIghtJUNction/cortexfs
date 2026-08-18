# FUSE 投影

`/ctx` 是 FUSE ABI 视图。后端故意不是 ABI。

第一次实现可以使用普通的本地状态：

```text
~/.local/share/cortexfs/
  objects/
  sessions/
  logs/
  runtime/
```

将状态的 FUSE 项目投射到 `/ctx`。动态文件可能表现得像 `/proc`；
耐用文件可以由普通文件支持。客户端不必关心是否
路径来自本地文件、生成的运行时状态或后续的后端。

FUSE 投影应保持小型：

```text
readdir                        读取目录
getattr                        查询属性
read                           读取文件
write small control files      写入小型控制文件
atomic replace                 原子替换
controlled agent lifecycle creation   受控代理生命周期创建
remove empty durable user/shared directories   清理空的持久 user/shared 目录
read-only executable object projection     只读可执行对象投影
Unix socket path projection              UNIX socket 路径投影
session files                           会话文件
read-only CortexFS extended attributes   只读 CortexFS 扩展属性
```

不要将这些添加到 FUSE 投影中：

```text
 分布式后端
 数据库后端
 向量存储
 集群运行时
 provider 注册表
 高层工作流运行时
 热重载命令
 以 watcher 形式出现的 ABI
```

开发触发的行为在文件系统 ABI 之外。Git 提交是
仅项目开发触发边界；不要添加根级作业、钩子或
工作流入口。

## 动态且耐用

路径语义保持简单：

```text
status                                 动态
model/<provider>/<model>               动态可执行入口
model/<provider>/<model>.sock          动态 socket；存在表示 session=socket
model/<provider>/<model>.d/status      动态
model/<provider>/<model>.d/log         动态或持久（实现可选）
model/<provider>/<model>.d/id          持久或配置投影
agent/<name>                           动态可执行入口
agent/<name>.sock                      动态 socket
agent/<name>.d/status                  动态
agent/<name>.d/pid                     动态
agent/<name>.d/owner                   持久
agent/<name>.d/uid                     持久
agent/<name>.d/gid                     持久
agent/<name>.d/groups                  持久
agent/<name>.d/label                   持久
agent/<name>.d/iso                     持久
agent/<name>.d/parent                  持久
agent/<name>.d/root                    持久
agent/<name>.d/cwd                     持久
agent/<name>.d/env                     持久
agent/<name>.d/path                    持久
agent/<name>.d/mount                   持久
agent/<name>.d/model                   持久
agent/<name>.d/system.md               持久
agent/<name>.d/abi                     必需可执行启动 ABI：sdk-envelope-v1
agent/<name>.d/policy                  持久
agent/<name>.d/log                      动态或持久（实现可选）
tool/<name>                            动态可执行入口
tool/<name>.d/schema                   持久
home/<uid>/model/*                     持久别名或用户模型入口
home/<uid>/tool/*                      持久用户工具
home/<uid>/agent/*                     持久用户代理状态
home/<uid>/                           持久
shared/<name>/                         持久
```

客户端不能依赖后端实现细节。

代理可见工具视图可能是基于内存的动态投影；
由系统、用户和共享工具源层构成。该投影不是持久状态，不应通过
写占位符文件或默认文件表达，也不应通过符号链接到
`home/<uid>/tool` 来模拟持久状态。

## 受控代理生命周期写入

FUSE 将 `agent/` 作为模式 `01777` 暴露，所以 `default_permissions` 允许一个
无权限的所有者开始创建代理。后备 `agent/` 目录保持
它的原始模式。此预期权限不授予一般写入权限：

```text
agent/<name>.d/                         所有者可写的控制目录与钩子骨架
agent/<name>.d/.<control>.tmp-...       原子控制临时文件
agent/<name>                            所有者可写的可执行包装文件
agent/.<name>.tmp-...                   原子包装文件临时文件
agent/<name>.sock                       所有者可写的 socket 占位符或运行时别名
home/<uid>/agent/<name>/...             文档化的代理 home 骨架
```

请求的 uid 必须与 `home/<uid>` 和 `agent/<name>.d/owner` 匹配。在此之前
`owner` 控制存在，新的普通控制目录必须归属
请求 uid。原子重命名仅限同一目录，接受生成的
`.<target>.tmp-<pid>-<nonce>-<attempt>` 形状，并且可能仅针对匹配的目标
已知的控制文件或包装器。符号链接、转义路径、其他用户的代理，
未知控件，以及 `agent/` 或 `home/` 下的任意文件失败。

代理控制目录只允许其所有者 UID 写入。CortexFS
因此处理共享的进程
将 UID 作为一个安全主体。在 FUSE 上，
源自路径的合成 inode 编号无法在原子操作中进行比较
临时路径及其目标；其余相同 UID 的丢失更新窗口不在
跨 UID 授权边界，不授予其他所有者访问权限。

## 运行时套接字别名

Agent start 将实时套接字绑定到 `/run/user/<uid>/cortexfs/` 以下并保持
仅限 FUSE 支撑树中这些所有者授权的别名：

```text
agent/<name>.sock
  -> /run/user/<uid>/cortexfs/agent/.../<name>.sock

home/<uid>/agent/<name>/session/<session>/terminal/main.sock
  -> /run/user/<uid>/cortexfs/terminal/<name>/<session>/main.sock
```

目标必须是绝对的，保持在匹配的 uid 运行时前缀以下，并且
匹配可见的代理/会话名称。别名父项必须在不跟随符号链接时打开。
创建、替换、取消链接需要代理所有者 UID。该流程
停止的宿主创建的代理可能保留真实套接字占位符；启动会替换
它与运行时别名一起。当任一情况发生时，在记录 `ready` 之前启动必须失败
无法使用 `readlink` 创建和验证可见别名。

一些部署也保持始终开启
系统代理套接字作为直接套接字
节点位于 `/ctx/agent/<name>.sock`，而不是指向 `/run/user/...` 的符号链接。
两种表示方式都是有效的：运行时可能直接暴露套接字节点，或者
通过该代理的所有者授权的符号链接。

## 扩展属性

FUSE暴露只读`user.cortexfs.*`扩展属性，以便代理可以
在读取完整内容之前检查路径：

```text
user.cortexfs.abi_path               相对于 /ctx 的 ABI 路径
user.cortexfs.kind                   稳定的 ctx.* 路径分类
user.cortexfs.origin                 virtual、disk 或 overlay
user.cortexfs.storage                memory 或 disk
user.cortexfs.virtual                true 或 false
user.cortexfs.backing_exists         true 或 false
user.cortexfs.backing_path           后端实现路径（若存在）
user.cortexfs.bytes                  预估字节大小
user.cortexfs.token_estimate         用于读取规划的快速 token 估计
user.cortexfs.input_token_estimate   读入 context 时估计的输入 token
user.cortexfs.output_token_estimate  估计的输出 token；未知时为 0
user.cortexfs.cache_bytes            CortexFS 已知缓存字节；无缓存为 0
user.cortexfs.cache_entries          缓存项数量；无缓存为 0
user.cortexfs.cache_state            none、partial、warm 或 stale
user.cortexfs.tokenizer              分词器/估算器 ID
```

`origin=virtual storage=memory` 意味着该文件是由 CortexFS 投影的
而不是从耐用的备份文件中读取。`origin=disk storage=disk` 意味着
可见内容来自后端文件系统。`origin=overlay
storage=memory` 用于运行时覆盖，例如实时套接字路径。

除非运行时后来写入了精确的分词器，否则令牌计数都是估算值
元数据。默认估计器是`byte-estimate-v1`，一种廉价的先读后读
不扫描完整文件内容的启发式方法。这些扩展属性不是控制属性
文件；`setxattr` 和 `removexattr` 必须失败。

## 目录删除

FUSE 仅支持 `rmdir` 用于
空的耐用普通目录下
`home/<uid>/...` 和 `shared/<space>/...`。

它不得移除 `/ctx`、顶级 ABI 目录、全局对象投影，
虚拟路径、套接字、符号链接或非空目录。非空目录
在 `ENOTEMPTY` 上失败；只读 ABI/投影路径在 `EROFS` 上失败。
