# Verus 证明

CortexFS 将 Verus 证明保存在运行时 Cargo 工作区之外。Verus 是一个
静态验证器作为单独的 `verus` 二进制分发，因此普通的 `cargo
test` 和 `cargo build` 不依赖它。

使用以下命令运行验证套件：

```sh
scripts/verify-verus.sh
```

这个测试工具已使用上游 Verus 版本进行检查
`release/0.2026.06.20.911e4e7`。

## 当前覆盖范围

当前的证明目标是`proofs/verus/abi_name.rs`。它定义了
独立的 `is_valid_object_name` 规范谓词，用于稳定的 64 字节
`docs/spec/object-abi.md` 的对象名称规则：

```text
[a-zA-Z0-9][a-zA-Z0-9._+-]{0,63}
```

除了语法之外，谓语还拒绝保留的`.sock`和
`.d` 后缀。它的证明功能为名称建立这些安全性事实
独立谓词接受：

```text
the name is non-empty
the name is at most 64 bytes
the first byte is ASCII alphanumeric
all bytes are ASCII path-component bytes
NUL, newline, and slash cannot appear
.sock and .d control suffixes are rejected
```

可执行文件 `is_object_name` 的实现位于
`crates/cortexfs/src/abi/path.rs`；它的64字节限制是
`MAX_OBJECT_NAME_LEN` 在 `crates/cortexfs/src/abi/constants.rs`。沃鲁斯
谓词目前手动反映了该逻辑。没有证据将其联系起来
独立规范到可执行的 Rust im
实施。

只有当`scripts/verify-verus.sh`运行成功时才会检查证明
在 `PATH` 上使用兼容的 `verus` 二进制文件。普通 Cargo 命令不会检查
它，而且仓库 CI 目前不会调用该脚本。

## 升级边界

以下仍属于未来的验证工作，而不是当前的证明覆盖范围：

- `is_valid_object_name` 与可执行 Rust 之间的等价性
  `is_object_name`
- `is_model_name`中的提供者/模型组合
- `is_model_reference` 接受的规范别名
- `is_object_name_for_class`中的类相关验证
- 在 `crates/cortexfs-agent-sdk/src/lib.rs` 中的 SDK 本地谓词，该
目前使用的是 255 字节的限制，而不是核心 ABI 的 64 字节限制
- CI 对独立 Verus 装置的强制执行

对谓词、实现和规范 ABI 进行明确审查
当对象名称规则发生变化时。 SDK 限制不匹配的问题将无法解决，直到
可执行谓词是对齐的，或者它们的差异已被指定。
