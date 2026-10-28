# 重建 agent.sh 薄客户端

重建一个兼容 `REFERENCE.md` 所列公开调用的可选 CortexFS 前端。默认选择 Bash，
使已有 `./agent.sh` 调用成立；可选择其他语言，前提是保持同一可执行入口、参数、
UTF-8 与进程行为，不强迫用户安装 provider SDK 或另一个运行时数据库。

`REFERENCE.md` 是固定快照的用户文档。保留无 prompt 进入 `ctx agent chat AGENT`、
有 prompt 进入 `ctx agent send AGENT` 的区别，保持 prompt 各参数的边界与特殊字符。
支持 `--chat`、`--repl`、`--attach`、`--watch`、`--session`、`--resume`、`--history`、
`--pack`、`--tools`、`--children`、`--cancel`、`--raw` 与 `CTX_BIN`。
参照父产品 ctx-coreutils 合同核对对应参数，未知或冲突选项必须有明确错误。

`CTX_BIN` 显式指定时使用该精确程序；未指定时保留文档中的开发 checkout 候选顺序
以及最后从 PATH 选择 ctx 的行为。开发路径由客户端所处位置确定，不依赖本配方
仓库具有 target/。没有任何可用 ctx 时报告缺少前提且非零退出。

所有 socket IO、line editing、response rendering、session 选择、terminal attach、
认证和 policy 决策交给 ctx。客户端不直接连接 provider，不保存私有历史，
不根据 skill、prompt 或消息授予权限，不增加 /ctx 根目录。
CTX_ROOT、CTX_HOME、CTX_PATH、session index/current 与 default 的含义按
REFERENCE 保留。只读观察不得偷偷变为终端输入或启动第二个 Agent。

中文和英文正文由 ctx/终端提供，客户端保持 UTF-8 原文；新增诊断至少清楚说明
失败对象和处理方法。stdout/stderr、非 TTY、取消与子进程退出状态不得被装饰性包装
破坏。用合成 CLI 验证 argv、环境、IO 和退出码，再与父产品完成真实集成。
不得声称替身测试证明 /ctx 或 provider 行为。

应用递归依赖 quality 的 CLI、错误和代码质量合同；其 UI 条款仅在存在相应界面时
适用。没有耗时或内存基线，性能预算未知；测量启动和转发开销时记录环境、样本与命令，
不得编造阈值。成功与失败验收见 ACCEPTANCE.md。
