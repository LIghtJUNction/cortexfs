# 外部媒体登记

本配方没有图片、SVG、视频、二进制、base64 资产或下载/执行钩子。
原媒体已保留在主仓库 `docs/assets/` 和 `docs/assets/reconstruction-site/`；它们是
配方外的可选构建输入。精确复用 logo/demo/海报需要另行取得这些已授权媒体，不能
只凭 Markdown 声称原视频已随包分发或达到像素级一致。

原始来源固定为 `LIghtJUNction/cortexfs@84f349c97bc2f6a611109686a02e21de3800042a`。
下表从旧静态提取包的实际字节计算 SHA-256，并与原始 Git blob 核对；媒体没有播放
或浏览器视觉验收。视频若通过 Git LFS 分发，还需要 LFS 对象可用，Git 指针不算媒体。

| 原始路径 | 大小 bytes | SHA-256 |
| --- | ---: | --- |
| `docs/assets/cortexfs-abi-map.svg` | 8074 | `e2b9a25fea966decc7b7e6c8158e9233bf7d59665c9031abd622ef62028d600f` |
| `docs/assets/cortexfs-demo-poster.jpg` | 61751 | `5f3e8b7984a23f080b853fb434955e1bb1cac19290a1a877fa75c5a8e07c9264` |
| `docs/assets/cortexfs-demo.mp4` | 1172780 | `e673aac260af4657420057e449c3bb303550e8159563dcd315129aaef3439e89` |
| `docs/assets/cortexfs-demo.webm` | 986153 | `72ef9c92ae55b0924e8e0beb8b1f52bcf1352c5cb2b8828cc0e12e7d01f7a92f` |
| `docs/assets/cortexfs-hero.svg` | 754 | `c94f71a6a251764fdf64cb47cf8a889be4c2401b6afd007429d0a405ba1268ad` |
| `docs/assets/cortexfs-logo.jpg` | 41658 | `84a9dfe154290e643ad9ca15780d1569bc27bb6278a5aa9bbcd98bfc207a781d` |
| `docs/assets/cortexfs-performance.svg` | 4671 | `decbfc0d9b225802304be5c101109e9b8173b39bdd5b65306f61b968e92dc200` |
| `docs-site/static/img/cortexfs-favicon.svg` | 676 | `68f632ea99ebfca6eac4f301e870a327a00ee99f956b9082cb8c225385f2eaa8` |
| `docs-site/static/img/cortexfs-logo.svg` | 701 | `045e5f3f26bcc5706ddfa65739e074bc4d1d28bc10537e777a2e8b21cecebcb8` |
| `docs-site/static/img/cortexfs-social-card.png` | 75763 | `a4b9b3f87a03445a777626bee01848b607fc14c2ca170d8a7f8de24b9319d3b5` |
| `docs-site/static/img/cortexfs-social-card.svg` | 4292 | `7b5f63c36ed5f9f149212bac8f9aadc6cabbfb5866c1b0d600477ae5fefba4e0` |
| `docs-site/static/video/cortexfs-demo-poster.jpg` | 61751 | `5f3e8b7984a23f080b853fb434955e1bb1cac19290a1a877fa75c5a8e07c9264` |
| `docs-site/static/video/cortexfs-demo.mp4` | 1172780 | `e673aac260af4657420057e449c3bb303550e8159563dcd315129aaef3439e89` |
| `docs-site/static/video/cortexfs-demo.webm` | 986153 | `72ef9c92ae55b0924e8e0beb8b1f52bcf1352c5cb2b8828cc0e12e7d01f7a92f` |

精确媒体复用模式：验证来源、许可和上述 hash 后由构建者显式接入网站；媒体缺失时
页面必须提供清楚的不可用说明和 fallback，不能指向不存在的包内 assets/。该模式的
原 demo 验收应标记受阻，不能用占位内容冒充通过。

仅配方模式：按 `contracts/docs/DESIGN.md`、`contracts/site/CONTENT.md` 重建版式与
交互，用原创且明确标为合成的示例说明功能。此模式允许独立离线重建产品和站点交互，
但原视频、图像一致性与外部 YouTube 播放均未验证。不得将新合成录屏称为原版演示。

原站 introduction 使用 YouTube 视频 ID `3BhCiHWbrUQ` 的隐私增强嵌入；它是可选外部
网络资源，不是离线构建依赖。不要自动下载或执行远程资料。

文档中指向 `assets/`、旧仓库源码、`docs-site/`、benchmark JSON 或历史构建脚本的
链接只是历史引用；对应资产依本文件处理，实现/benchmark/脚本不在配方内。
这些缺省不影响 Markdown 配方闭包的结构校验，但不能据此宣布完整媒体体验通过。

许可：本配方文档与原 CortexFS 包保留 MIT 署名，见 LICENSE.md；外部服务内容和
视频嵌入仍受各自许可约束，本次未做额外权利授权。
