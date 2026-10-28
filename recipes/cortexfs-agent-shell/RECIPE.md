# CortexFS Agent 薄客户端

```toml
format = "recipkg/1"

[package]
name = "cortexfs-agent-shell"
version = "0.1.0"
kind = "software"
description = "通过 ctx agent CLI 操作 CortexFS 的可选 agent.sh 薄客户端。"
entry = "BUILD_PROMPT.md"

[dependencies.quality]
source = "../software-quality"
```

观察目标为 CortexFS 基线引用的 `agent.sh` 提交
`2226376cae430b96a6298b7a786d38be689a1bfb`。源仓库身份、可获得证据与未知项
见 `OBSERVATIONS.md`。不要求访问其源码或原脚本才能重建。

`ctx` 是由父产品集成提供的执行接口，不是本叶子配方指回父配方的依赖；单独验收时
可用记录参数与退出码的合成 CLI 替身，集成时必须使用重建后的真实 `ctx`。
`quality` 是唯一配方依赖，约束 CLI 可读性、错误处理与实现质量。
