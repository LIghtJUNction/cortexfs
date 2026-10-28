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

## 新工作区与远程边界

任务分支 `e90302251977335266a630d7fbbc40dc83497b70` 已推送并创建 PR #342。GitHub recipkg topic 已读回，
原 agent/fuse/linux topics 保留。首次远程递归 clone 连接停滞；改用 HTTP/1.1 后
仍在 discover-dependencies 子模块遇到 curl 28/expected packfile，在限时内中止。
因此不宣称完整 GitHub 递归克隆通过。两个失败尝试的临时目录均已删除。

随后从本地固定分支浅克隆到全新临时工作区，使用本次已从 canonical GitHub 取得并
核对的 11 个技能 checkout 作本地传输镜像，验证全部 gitlink 可初始化、固定提交
一致、checkout 干净，三个 recipe check 均通过。镜像仅用于该临时工作区传输，
提交中的 .gitmodules 仍为 canonical GitHub URL。该结果验证隔离克隆与 pin 完整性，
不替代成功的全远程网络下载。临时镜像 checkout 已删除。

克隆均设置 GIT_LFS_SKIP_SMUDGE=1；媒体不在配方内，未验证媒体下载/播放。
旧本地观察目录和原 .git/modules 恢复材料保留。
