# Recipkg 本地交付验证

日期：2026-10-29。验证的是配方结构与完整上下文，没有运行产品。

- recipkg 0.1.0：三个独立 recipe check 全部通过。
- 根配方临时 pack、归档 check、完整中文 build prompt 渲染通过。
- 逐成员检查归档只含 UTF-8 Markdown，无 NUL、路径穿越、绝对路径、链接或执行文件。
- 逐文件核对全部归档 Markdown 正文进入完整渲染提示词。
- 核对 21 个 REQ、12 个 IMP、两级 quality 依赖和归档 LOCK.md。
- 内置 quality 的三个主文件与固定上游提交逐字一致。
- 主配方 Markdown 链接目标存在；资产仅在外部登记，不含媒体或实现。

| 指标 | 实际值 |
| --- | ---: |
| unique_recipes | 3 |
| materialized_recipe_nodes | 4 |
| markdown_payload_files | 118 |
| payload_bytes | 886615 |
| archive_bytes | 329016 |
| rendered_prompt_bytes | 901395 |

临时归档 SHA-256：`241f6af2e899b4097b576b342c0c5ad9bd293213e3e1f2c2a9ecc95e42217547`。
临时归档和完整 prompt 在检查结束后由临时目录自动删除，不作为持久交付物。

技能与远程交付另核对 .gitmodules、160000 gitlink、canonical URL、固定提交、
clean state、可读 SKILL/资源，并在推送后验证递归克隆。产品 FUSE/CLI/真实服务、
安装回滚、浏览器、性能和独立重建均为 not-run；媒体原件模式另需外部资产。

最终检查额外确认：全部 recipe 相对 Markdown 链接可解析，归档文件无执行权限；
11 个官方技能的 gitlink/remote/HEAD/clean state 一致，原有自定义技能与起始提交逐字一致。
