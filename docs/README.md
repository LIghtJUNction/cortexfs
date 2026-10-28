# 文档来源与状态

`docs/` 保留到上游 `7261cb399ba1f8efa387736938ea3d8eec1a0acb`（2026-09-30）
的 canonical 文档，含 hosted Agent CLI 迁移方向。产品实现、测试、构建安装代码、
评估运行器和网站实现已移除，历史命令不能在当前分支执行。

## Recipkg 重建配方

入口是 [`RECIPE.md`](../recipes/cortexfs/RECIPE.md) 与
[`BUILD_PROMPT.md`](../recipes/cortexfs/BUILD_PROMPT.md)。基线固定为
`84f349c97bc2f6a611109686a02e21de3800042a`（v0.1.21-10-g84f349c9），
重建时以配方自带 contracts/docs/ 为准。后续 hosted CLI 设计不是隐含要求。

旧 cortexfs-prompts 远程已不存在，原 gitlink 已移除。本地旧目录保留并忽略；
Markdown 合同现在随 recipes/ 跟踪，媒体位于配方外 docs/assets/。
格式/闭包/临时归档检查不能证明产品独立重建成功；验收状态见
[`ACCEPTANCE.md`](../recipes/cortexfs/ACCEPTANCE.md)。

## 规范与翻译

详细 spec/ 优先于摘要、旧宣传和历史翻译；稳定根为 status、bin、model、agent、tool、
channel、home、shared。冲突需明确记录，不自行扩展 ABI。
translations/zh-Hans/ 只保留真实译文，未翻译页回退 canonical 英文，不复制占位。
网站实现已移除。配方内快照和这里较新文档的版本边界须始终保持清楚。
