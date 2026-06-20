不要使用mod.rs
请使用cargo add新增依赖，不要手动编辑文件添加依赖
先去阅读规范（如有）：docs/DESIGN.md
开发触发事件以 Git commit 为唯一边界；不要新增后台监听、轮询或热加载子命令。
文件系统 ABI 只使用当前规范里的短单数顶层目录；不要新增 chan/job/hook/workflow 这类第二套提交或编排入口。
统一提交语义是：写临时文件，同目录原子 rename 成 `*.req.json`，从 outbox 读取结果，向 audit 追加事实。

测试约定：
- FUSE 集成测试挂载点固定使用 `tests/mounts/cortexfs`。
- 该目录只作为本地测试挂载点，不要在其中放源码、fixture 或持久化数据。
- provider/model 设计必须保持中立；不要把 Ollama 写成核心默认路径、核心能力或特殊分支。
- 需要调用真实本地模型做 live test 时，当前测试 fixture 使用 Ollama 模型 `smollm2:135m`。
- 除本地轻量模型 fixture 外，用户也会使用自己配置的 provider/本地聚合 API 做 live test；这类测试必须走现有 provider registry、route、secret 状态和统一提交语义，不要把具体供应商写成核心默认路径或特殊分支。
- 默认 live test 不依赖外部云 API；如果 `smollm2:135m` 不存在，先提示用户安装/拉取，不要静默换模型。只有用户明确要求测试自己配置的供应商/聚合 API 时，才使用该已有配置。
