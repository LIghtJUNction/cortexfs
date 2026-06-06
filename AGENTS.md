不要使用mod.rs
请使用cargo add新增依赖，不要手动编辑文件添加依赖
先去阅读规范（如有）：docs/DESIGN.md
开发触发事件以 Git commit 为唯一边界；不要新增后台监听、轮询或热加载子命令。

测试约定：
- FUSE 集成测试挂载点固定使用 `tests/mounts/cortexfs`。
- 该目录只作为本地测试挂载点，不要在其中放源码、fixture 或持久化数据。
- provider/model 设计必须保持中立；不要把 Ollama 写成核心默认路径、核心能力或特殊分支。
- 需要调用真实本地模型做 live test 时，当前测试 fixture 使用 Ollama 模型 `smollm2:135m`。
- live test 不依赖外部云 API；如果 `smollm2:135m` 不存在，先提示用户安装/拉取，不要静默换模型。
