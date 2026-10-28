# 文档来源与状态

`docs/` 是本仓库保留的 canonical 文档。它记录到上游 commit
`7261cb399ba1f8efa387736938ea3d8eec1a0acb`（2026-09-30），其中的规范反映 hosted Agent CLI
迁移方向。当前工作树中的产品实现、测试、示例、安装/构建代码、评估运行器和文档站点已
移除；历史命令不能在本分支执行。

## 提示词快照

重建提示词位于 [`cortexfs-prompts`](cortexfs-prompts/README.md) 子模块，入口为
`BUILD_PROMPT.md`。它提取自任务开始时的本地源码 commit
`84f349c97bc2f6a611109686a02e21de3800042a`（`v0.1.21-10-g84f349c9`），比此处保留的文档早。
重建该源码快照时，以提示词包自带的 `contracts/docs/` 和证据说明为准；后续 hosted CLI
设计属于 `docs/` 的更新，不是那个提示词的隐含要求。

提示词由静态文档与源码检查生成，未启动或重建原版。其 SHA-256 清单可检查包内文件是否
变化；这不表示独立重建已通过。

## 规范与翻译

`spec/` 中详细 ABI 优先于摘要、旧宣传文案和历史翻译。稳定根目录为 `status`、`bin`、
`model`、`agent`、`tool`、`channel`、`home`、`shared`。若规范有冲突，记录具体差异，不
自行扩展 ABI。

简体中文翻译存放在 `translations/zh-Hans/`。这里只保留真实译文；未翻译页面应链接到
`docs/` 中的 canonical 文档，不复制英文占位内容。文档站点实现已从跟踪树移除。
