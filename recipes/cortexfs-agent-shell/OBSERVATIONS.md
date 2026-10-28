# 观察与来源

- 2026-10-29 在 CortexFS 文档仓库执行 Recipkg 迁移，未运行原版或重建版。
- 父源码基线：`84f349c97bc2f6a611109686a02e21de3800042a`。
- 子模块：`https://github.com/LIghtJUNction/agent.sh.git`，gitlink
  `2226376cae430b96a6298b7a786d38be689a1bfb`，已从父提交的 `.gitmodules` 核对。
- `REFERENCE.md` 来自此前静态提取包
  `3b17bed77b134e7b5db06c9baee89e412aca1def` 的
  `analysis/source-docs/agent-sh-README.md`；本次实际读取了它并核对父 gitlink。
- 本次没有检查子模块的脚本正文或单独许可证，无法确认所有原始错误文案、信号处理
  和边缘选项组合。把它们作为待实现和待验证的合同，不声称已观察运行。
- `LICENSE.md` 适用于本配方文字，沿用 CortexFS 提取包的 MIT 许可；不据此断言
  未复制的原 `agent.sh` 实现采用同一许可。独立重建不得复制未审查的原实现。
- 外部前提：Linux shell/进程环境，Bash（默认实现选择）和父产品提供的 ctx。
  没有独立 UI、服务账户、数据集或源码仓库依赖。
