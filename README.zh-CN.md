# CortexFS

通过 Linux `/ctx` 文件系统访问 Agent 运行时。

[English](README.md) · [文档目录](docs/translations/zh-Hans/README.md) · [重建配方](recipes/cortexfs/RECIPE.md)

本仓库保存**文档、ABI 契约和重建配方**。产品源码、安装器、测试和文档网站实现已移除；
指南中的产品命令用于查阅历史行为。

## 从这里开始

| 我想要…… | 阅读 |
| --- | --- |
| 理解接口如何工作 | [架构](docs/translations/zh-Hans/architecture.md) |
| 重建产品 | [配方](recipes/cortexfs/RECIPE.md) → [构建任务](recipes/cortexfs/BUILD_PROMPT.md) → [验收要求](recipes/cortexfs/ACCEPTANCE.md) |
| 查询路径或协议 | [ABI 规范目录](docs/translations/zh-Hans/spec/README.md) |
| 查找使用指南 | [文档目录](docs/translations/zh-Hans/README.md) |

## 使用当前仓库

单独安装 [Recipkg CLI](https://github.com/recipkg/recipkg) 后，在仓库根目录执行：

```sh
recipkg check recipes/cortexfs
recipkg build recipes/cortexfs --lang zh-CN
```

`check` 校验配方结构和引用；`build` 输出完整的递归实现任务，供构建者执行。
产品编译、运行测试和验收需要另行完成。

配方包含两个本地依赖：可选薄客户端 `cortexfs-agent-shell` 和共享工程要求
`software-quality`。主机前提、媒体与性能目标分别见
[环境](recipes/cortexfs/ENVIRONMENT.md)、[资产](recipes/cortexfs/ASSETS.md)和
[性能](recipes/cortexfs/PERFORMANCE.md)。

## 两个文档版本

| 材料 | 范围 |
| --- | --- |
| [`recipes/cortexfs/`](recipes/cortexfs/RECIPE.md) | 使用包内契约重建源码快照 `84f349c`。 |
| [`docs/`](docs/translations/zh-Hans/README.md) | 保留至 `7261cb3` 的产品文档，包含后续托管 Agent CLI 设计。 |

重建时分别使用对应版本。[文档目录](docs/translations/zh-Hans/README.md#版本与依据)
提供完整的来源引用。[研究记录](recipes/cortexfs/RESEARCH.md)和
[改进要求](recipes/cortexfs/IMPROVEMENTS.md)列出新增验收目标；产品验证需另附执行结果。

## 参与维护

修改前阅读 [AGENTS.md](AGENTS.md)。ABI 细节集中维护在规范中，指南通过链接引用，
修改通过 PR 交付。共享技能及固定子模块见 [Recipkg 接入说明](.agents/RECIPKG.md)。

[MIT 许可](LICENSE) · [配方署名与许可](recipes/cortexfs/LICENSE.md)
