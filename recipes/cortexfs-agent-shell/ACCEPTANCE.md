# Agent 薄客户端验收

状态：只完成配方整理，没有生成或运行客户端。以下均是重建者应执行的检查。

| ID | 输入/条件 | 可观察结果 |
| --- | --- | --- |
| SH-01 | agent.sh coder；agent.sh coder "fix tests" | 前者转发 chat，后者转发 send；Agent 名和完整 prompt 不变。 |
| SH-02 | prompt 含中文、空格、换行、引号、美元符号 | 合成 CLI 记录的参数与输入逐项一致，无 shell 再解释。 |
| SH-03 | 显式 CTX_BIN，同时 PATH 有其他 ctx | 只使用指定程序；缺失/不可执行时清楚报错，非零退出。 |
| SH-04 | 覆盖开发 debug/release 候选与 PATH 回退 | 顺序与 REFERENCE 一致；全部缺失时不伪报成功。 |
| SH-05 | REFERENCE 中每个公开选项及 --raw | 调用对应 ctx 行为；输出/错误流、退出码保持，未知或冲突输入被拒绝。 |
| SH-06 | 显式 session；current 存在/缺失 | 由 ctx 选择指定/current/default session；不创建客户端私有历史。 |
| SH-07 | --watch、--attach、cancel、子进程失败或中断 | watch 保持只读；attach/cancel 转发；终端与信号资源正确释放。 |
| SH-08 | 与父产品真实 ctx、合成 provider 集成 | chat/send/history/raw/attach 读取同一 durable session，无第二套 socket/权限/历史。 |
| SH-09 | 无 provider 凭据、恶意 Agent 名/选项、非 TTY | 无 provider 直连、命令注入或静默成功；错误可理解，UTF-8 不损坏。 |

单独验收使用全新临时目录和记录 argv 的 CLI 替身，不需要真实账户。SH-08 必须另外
运行重建后的完整产品。应用 quality 并记录实际结果、日志、环境与未执行项；真实
provider 和性能都尚未验证。
