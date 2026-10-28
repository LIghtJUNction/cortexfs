# 环境、语言与外部前提

## 本次选择

沿用现有 CortexFS 仓库名称，在同仓库 `recipes/cortexfs/` 维护主配方，
`recipes/cortexfs-agent-shell/` 和 `recipes/software-quality/` 是本地递归依赖。
原始实现只通过已固定的历史 Git 对象作观察；当前没有待搬运的产品实现，也不恢复源码。
未新增 source/ checkout，未把源码、原程序或历史提示词仓库声明为配方依赖。

重建者可选适合目标的语言；原工程 Rust 2024 / MSRV 1.91、Bash 和
TypeScript/React 是观察事实，不是 ABI 要求。若选择原 Rust 工具链，遵守该版本
约束；若换语言，公开路径、协议、可执行名称、调用、配置、权限和持久格式仍须兼容。
agent-shell 默认 Bash，可在兼容其入口和进程语义的条件下替换。

## 平台

Linux 是产品运行平台，要求 FUSE3、Unix 身份/权限/socket、PTY、systemd 用户与
系统服务，以及 bubblewrap 0.10+ 所需主机能力。Arch、Debian/Ubuntu、Fedora/RHEL、
openSUSE/SLES 是历史指南声明的发行版家族；包元数据声明 x86_64、aarch64。
没有这些家族每个发行版的最低版本或实机结果，不扩张为 Windows/macOS 原生支持，
也不把任意容器内的 namespace/FUSE 可用性当作必然。

文档网站面向桌面与窄屏浏览器。原 Node >=20 与 Docusaurus 3.10.1/React 19 属于
历史实现选择；新实现只需保持文档及站点外部体验。具体最低浏览器版本未知。

## 语言与格式

- canonical ABI 为英文，保留已有简体中文译文；没有译文时明确回退 canonical 英文，
  不用复制英文伪装中文翻译。新需求解释与导航需同步维护英中两种语言。
- CLI/JSON/JSONL、文件名和消息使用 UTF-8；没有 LANG/LC_ALL 时明确选择可用的
  UTF-8 环境或给出可操作诊断，不悄悄损坏中文。
- 时区不是 Agent 身份或 session 隔离依据；时间戳遵守各 ABI 指定格式。
  未指定的显示时区应明确记录，不能偷偷更改协议时间字段。没有新增 RTL 需求，
  RTL 和其他 locale 支持均未验证。

## 外部前提分类

| 类别 | 前提 | 边界 |
| --- | --- | --- |
| 主机能力 | Linux、FUSE3、systemd、Unix socket/PTY、bwrap 0.10+、挂载及用户 namespace 能力 | 部署者提供；不在配方中重建操作系统。缺失时在改动主机前预检失败。 |
| 构建工具 | 所选语言编译器/运行时、链接与打包工具 | 构建者锁定真实版本；语言库不是未经提炼的配方依赖。 |
| 基础工具 | CA、curl、util-linux 等指南中的安装前提，必要的 Secret Service/libsecret 能力 | 配置与凭据分离；报告发行版差异，按实际所选 secret backend 验收。 |
| 远程服务 | provider、各 channel 平台、MCP server、可选 Future AGI | 先用本地合成端点与凭据验收；真实账户需独立验证，存在源码不等于 live 兼容。 |
| 品牌媒体 | 原 logo、poster、demo、social card 及外部视频 | 只作外部媒体前提，固定身份和校验见 ASSETS.md；无二进制进入配方。 |
| 测试资料 | 合成 UID、挂载、sessions、provider/model、消息、Git 仓库、受保护假 secret | 重建者在隔离工作区生成；不使用用户私有资料。 |
| 参考资料 | 已固定历史源码、当前较新的 docs/、研究中的同类项目 | 不构建依赖、不执行原程序、不复制未审查实现；研究案例不证明本产品有相同缺陷。 |

真实 provider key、付费账户、云主机或整个历史源码均不是本地基本验收的前提。
完整安装、媒体原件展示与 live 平台兼容须按各自前提单独报告。
