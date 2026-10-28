# CortexFS

![软件与依赖汇聚为可重建的配方](https://raw.githubusercontent.com/recipkg/recipkg/main/assets/recipkg-cover.png)

[English](README.en.md)

CortexFS 通过 Linux `/ctx` 文件系统公开 Agent 运行时行为。本仓库现以
**Recipkg 重建配方和 ABI 文档**为主，没有可构建的产品实现。

入口是 [`recipes/cortexfs/RECIPE.md`](recipes/cortexfs/RECIPE.md)，随后阅读
[`BUILD_PROMPT.md`](recipes/cortexfs/BUILD_PROMPT.md) 和
[`ACCEPTANCE.md`](recipes/cortexfs/ACCEPTANCE.md)。

## 重建目标与这轮改进

基线源码固定为 `84f349c97bc2f6a611109686a02e21de3800042a`
（`v0.1.21-10-g84f349c9`）。保留八项根 ABI、FUSE、model/agent/tool、durable
session、channel、CLI/PTY、扩展、Linux 安装/升级回滚与英中双语文档体验；可选合适的
实现语言，外部合同必须兼容。

[`RESEARCH.md`](recipes/cortexfs/RESEARCH.md) 记录 AgentFS、OpenHands、Sandbox Runtime、
Open WebUI 和本项目 issue 的反馈与状态；
[`IMPROVEMENTS.md`](recipes/cortexfs/IMPROVEMENTS.md) 将其转成 12 项新增改进/回归验收。
这些是重建要求，不能称为已经复现或修好的产品行为。

## 使用

另行安装 Recipkg CLI 后执行：

```sh
recipkg check recipes/cortexfs
recipkg build recipes/cortexfs --lang zh-CN
```

build 会打印完整递归重建任务，不会自动调用模型或执行构建。主配方依赖可选薄客户端
cortexfs-agent-shell 与 software-quality，客户端继续依赖同一 quality 叶子。
闭包全部为 UTF-8 Markdown；临时打包校验后删除归档，不在仓库保留发布包。

图片、视频在配方外；固定来源、hash 和缺失时的验收边界见
[`ASSETS.md`](recipes/cortexfs/ASSETS.md)。主机与账户等外部前提见
[`ENVIRONMENT.md`](recipes/cortexfs/ENVIRONMENT.md)，未测性能与预算见
[`PERFORMANCE.md`](recipes/cortexfs/PERFORMANCE.md)。

## 仓库布局与状态

- recipes/ 是主要输入，包含根配方及完整本地依赖。
- docs/ 的 canonical 文档截至较新的 `7261cb399ba1f8efa387736938ea3d8eec1a0acb`，
  含 hosted CLI 迁移方向；不能混入较早的重建基线。
- .agents/skills/ 保留已有项目技能，并用子模块固定 11 个 recipkg 官方技能，详见
  [接入说明](.agents/RECIPKG.md)。克隆时使用 git clone --recurse-submodules。
- docs/assets/ 是配方外参考媒体。失效的 cortexfs-prompts 远程 gitlink 已移除，
  原本地目录保留并忽略；Markdown 材料已纳入配方。

未来生成实现必须放在独立源码仓库，通过 generated/<小写短横线仓库名>/ 子模块固定；
本次没有生成实现。当前尚未完成独立重建、FUSE 实机、真实 provider/channel、各发行版
安装回滚、浏览器视觉或性能验收。历史指南中的程序不代表本 checkout 仍可运行。
原署名与 MIT 许可保留。
