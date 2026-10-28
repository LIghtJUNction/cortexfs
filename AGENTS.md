# CortexFS 配方与文档仓库说明

本分支以 Recipkg 配方、文档与 ABI 参考为主；产品源码、测试、示例、构建/安装代码、评估运行器和
文档站点实现已从跟踪树删除。历史指南中出现的源码路径和命令只描述对应快照，不代表
当前可执行。

## 文档来源

- `docs/` 是 canonical 源。详细 ABI 以 `docs/spec/` 为准；稳定根目录有八项，包含
  `/ctx/channel/`。
- 当前保留的文档截至上游 commit `7261cb399ba1f8efa387736938ea3d8eec1a0acb`，记录
  hosted Agent CLI 迁移方向。
- `recipes/cortexfs/` 是主要重建输入；基线为
  `84f349c97bc2f6a611109686a02e21de3800042a`，使用包内 contracts/docs/，不要混入
  后续 hosted CLI 文档。研究反馈的新增要求在 IMPROVEMENTS.md，与原版保证分开。
- 旧 `docs/cortexfs-prompts/` 的远程已不存在，gitlink 已移除；本地观察目录保留并忽略。
  配方是 UTF-8 Markdown-only，媒体和 Git/技能元数据不得进入配方负载。
- `docs/translations/zh-Hans/` 只保留真实中文译文。英文 canonical 文档缺少译文时直接引用
  `docs/` 对应页面，不增加相同英文占位文件。
- 标注为历史的评估、安装和开发文档可能引用已经删除的程序。不要把它们写成当前可运行或
  已重新验证的说明。

## 协作与 Git

- 文档修改在任务分支完成并通过 PR 交付。不要直接推送 `main`、强制推送或合并 PR，除非
  用户明确授权。
- 操作前检查工作区与子模块状态，保留已有改动；不要重置、清理或覆盖无关文件、缓存和
  本地媒体素材。
- 更新提示词直接修改 recipes/；运行 recipkg check、临时 pack、归档 check 和完整
  build prompt 渲染，验证后删除临时归档。除非用户要求，不交付持久归档。
- .agents/skills/ 的 11 个共享 Recipkg 技能是各自 canonical 仓库的固定子模块；
  保留原有自定义技能，先在上游技能仓库审查更新，再显式更新消费方 gitlink。
- 新增代码前先确认用户希望恢复实现源码；当前仓库不包含可构建的产品实现。
- 如另获授权生成实现，generated/ 只作容器，实际源码放在独立仓库并以
  generated/<同名小写短横线源码仓库>/ 子模块固定；不提交平铺实现，不将源码列为配方依赖。
