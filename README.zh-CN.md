# CortexFS

**CortexFS 正在重新确定产品方向。** 目前没有获批的新产品定位，当前仓库也不包含可构建的产品实现。

[English](README.md) · [项目范围](PROJECT_SCOPE.md) · [历史文档](docs/translations/zh-Hans/README.md)

此前的 Agent 运行时、Linux FUSE /ctx 接口、进程沙箱、模型与会话系统、消息通道和 MCP 基础设施，均已退出当前产品范围。相关文档仅作为历史资料保留，不再是路线图或实现要求。

## 当前工作

目前只进行问题发现和小规模可行性验证。编写代码前，必须先确定一个反复发生的用户任务，试用现有工具，定义最小首次使用体验，并通过 [PROJECT_SCOPE.md](PROJECT_SCOPE.md) 中的继续/停止条件。

当前没有可用的产品构建或安装命令。不要把旧重建配方当作下一代产品的设计依据。

## 仓库内容

- [PROJECT_SCOPE.md](PROJECT_SCOPE.md)：当前范围、非目标和决策门槛。
- [docs/](docs/translations/zh-Hans/README.md)：归档的旧设计与 ABI 文档。
- [recipes/](recipes/cortexfs/RECIPE.md)：早期产品快照的历史重建材料。
- [AGENTS.md](AGENTS.md)：产品重置期间的协作规则。

## 参与维护

保持改动小而明确。未经具体问题验证，不恢复已退役的子系统，也不增加通用抽象。所有改动在任务分支完成并通过 PR 交付，不直接推送 main。

[MIT 许可证](LICENSE)