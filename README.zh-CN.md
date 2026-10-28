# CortexFS

[English](README.en.md)

本仓库现作为 CortexFS 的文档与 ABI 参考。产品实现、测试、示例、构建/安装
脚本和文档站点实现已从当前源码树移除。保留的操作指南描述 `v0.1.21` 源码
快照，只作为参考，不能在当前 checkout 中照着构建或安装。

## 重建提示词

完整、自包含的重建提示词由
[`cortexfs-prompts` 子模块](docs/cortexfs-prompts/README.md)维护，入口是
[`BUILD_PROMPT.md`](docs/cortexfs-prompts/BUILD_PROMPT.md)。提示词包记录了源版本、
证据、文档冲突和文件哈希；它尚未经过独立重建验证。

## 规范文档

- [规范性 ABI 索引](docs/spec/README.md)
- [系统架构与产品边界](docs/architecture.md)
- [文档状态与来源](docs/README.md)
- [视觉设计规范](docs/DESIGN.md)
- [简体中文翻译](docs/translations/zh-Hans/)

详细根 ABI 定义 8 个稳定条目，其中包括 `/ctx/channel/`。旧版宣传文案仍可能写
“7 个根目录”；差异见提示词包的证据记录。

## 历史产物

若 `dist/` 中仍保留旧发布包，它们不是从当前文档仓库构建的版本。本仓库不再
提供受支持的源码构建或安装器。
