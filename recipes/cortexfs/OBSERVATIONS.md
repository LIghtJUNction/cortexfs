# 本次观察与来源

## 版本边界

- 迁移日期：2026-10-29，Asia/Taipei。
- 观察仓库：`https://github.com/LIghtJUNction/cortexfs.git`。
- 本次开始时父仓库：`52de681aeb1236d425f108314a20878bcdcd36f5`，工作区干净，
  main 与远程 main 一致。当前树只有文档、开发说明与提示词，不可构建产品。
- 重建基线：`84f349c97bc2f6a611109686a02e21de3800042a`，
  `v0.1.21-10-g84f349c9`。本次能读取其本地 Git 对象。
- 既有静态提取包：`3b17bed77b134e7b5db06c9baee89e412aca1def`，来源是旧
  `docs/cortexfs-prompts/` 子模块。2026-10-29 访问其原远程返回 Repository not found；
  本地材料仍在。此远程不是新配方的前提。
- 现有 `docs/` 更新至 `7261cb399ba1f8efa387736938ea3d8eec1a0acb`，
  描述较新的 hosted Agent CLI；它不能覆盖本配方内的较早固定合同。

## 本次实际静态检查

| 材料 | 实际读取的内容 | 支持的结论与限制 |
| --- | --- | --- |
| 基线 Cargo.toml | workspace、版本、MSRV、许可 | 原实现语言/版本选择；未编译，不能据此声称运行正确。 |
| cortexfs-paths/src/lib.rs | ROOT_ENTRIES 与路径校验常量 | 详细 ABI 确认八项根目录，动态单段长度 64；未挂载。 |
| ctx/main.rs | main、run 分派、错误输出 | CLI 错误边界；未运行帮助或全部子命令。 |
| scripts/install-linux.sh | MIN_RUST、MIN_BWRAP、locale、TTY 与 distro 检测 | 预检与双语流程的静态证据；未修改主机。 |
| packaging/arch/PKGBUILD | 架构、主机依赖、安装入口 | x86_64/aarch64 与依赖是元数据声明，非实机验收。 |
| docs-site/package.json、static/ 树 | 历史工具链、媒体 Git blob 标识 | 历史网站输入；未运行网站、截图或播放媒体。 |
| 基线 .gitmodules 与 agent.sh gitlink | 原 URL 与固定子提交 | 独立客户端的观察身份；未运行原脚本。 |
| 既有 BUILD_PROMPT、详细 spec、design/site、EVIDENCE 与 crate README | 编号需求、协议状态、边界、来源与未知项 | 复用先前静态提取，不能冒充本次逐行复核所有实现。 |

`analysis/EVIDENCE.md` 是 2026-10-28 的历史提取记录，保留其证据和未知项。里面
关于随包提供图片/视频与 SHA256SUMS 的描述仅适用于旧提取包；本 Recipkg 只包含
Markdown，替代媒体说明见 `ASSETS.md`，不包含旧清单或 JSON benchmark 文件。

## 新研究与推断

用户另外授权参考同类项目和高 star 项目反馈。本次读取公开 issue 正文与可获得的
讨论，将可迁移教训写入 `RESEARCH.md` 和 `IMPROVEMENTS.md`。star 只帮助挑选案例，
不证明实现、安全或需求正确；用户报告也不等于维护者确认，状态和结论须分开。
这些新增要求是本次改进合同，不是基线已实现、已复现的行为。

## 未运行与未知

未编译/启动原版或重建版、挂载 FUSE、调用模型、登录通道、执行安装升级回滚、
打开网站或测试辅助终端；没有可报告的产品性能基线、真实平台结果或像素一致性结论。
本次 Recipkg 格式/闭包/打包检查与未来产品验收是两件事，详见 `ACCEPTANCE.md`。
