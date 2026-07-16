use std::io::Read;

use crate::RuntimeClientError;

use super::{
    AGENT_INVOCATION_SCHEMA, AgentInvocationEnvelope, AgentToolObservation,
    MAX_AGENT_CONTEXT_BYTES, MAX_AGENT_INVOCATION_BYTES, MAX_AGENT_STEPS,
};

/// Maximum bytes read for a single invocation frame (payload + trailing newline).
///
/// Keeping this explicit limits allocator growth under malformed peer behavior.
/// Maximum bytes read for one invocation, including its newline.
///
/// Similar protocol framing:
/// - [Rust FS MCP architecture: stdin line-by-line JSON-RPC](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#request-lifecycle)
/// - [modelcontextprotocol/go-sdk discussion on newline transport](https://github.com/orgs/modelcontextprotocol/discussions/364)
const MAX_AGENT_INVOCATION_READ: u64 = 1024 * 1024 + 1;

/// Maximum byte budget for the tool result payload.
///
/// This protects host memory from oversized tool observations.
/// Maximum result bytes carried in one tool observation.
///
/// Truncation strategy parallels:
/// - [rust-fs-mcp truncation metrics](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#l557)
/// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
const MAX_OBSERVATION_BYTES: usize = 16 * 1024;

/// Reads and validates one newline-terminated invocation envelope frame from a stream.
///
/// 兼容策略：
/// - 一帧一行读入，`read_to_end` + 最大长度限制防止粘包/超大包导致阻塞。
/// - 长度与换行校验失败直接返回 `InvalidFrame`，避免跨协议边界污染状态机。
/// - 逐字段校验（schema/步骤/上下文长度/观察对象）是可复用的最小安全闭包。
///
/// 相关问题与实现参考：
/// - [modelcontextprotocol/servers#4206](https://github.com/modelcontextprotocol/servers/issues/4206)
/// - [modelcontextprotocol/servers#4207](https://github.com/modelcontextprotocol/servers/issues/4207)
/// - [modelcontextprotocol/servers#3505](https://github.com/modelcontextprotocol/servers/pull/3505)
/// - [modelcontextprotocol/servers#3402](https://github.com/modelcontextprotocol/servers/issues/3402)
/// - [modelcontextprotocol/go-sdk discussion #364](https://github.com/orgs/modelcontextprotocol/discussions/364)
/// - [rust-fs-mcp line based dispatch](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#request-lifecycle)
/// - [CortexFS PR #87](https://github.com/LIghtJUNction/cortexfs/pull/87)
/// - [modelcontextprotocol/python-sdk issue #262](https://github.com/modelcontextprotocol/python-sdk/issues/262)
pub fn read_agent_invocation(
    reader: impl Read,
) -> Result<AgentInvocationEnvelope, RuntimeClientError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_AGENT_INVOCATION_READ)
        .read_to_end(&mut bytes)
        .map_err(|_error| RuntimeClientError::CannotRead)?;

    if bytes.len() > MAX_AGENT_INVOCATION_BYTES
        || bytes.pop() != Some(b'\n')
        || bytes.contains(&b'\n')
    {
        return Err(RuntimeClientError::InvalidFrame);
    }

    let envelope: AgentInvocationEnvelope =
        serde_json::from_slice(&bytes).map_err(|_error| RuntimeClientError::InvalidFrame)?;

    if envelope.schema != AGENT_INVOCATION_SCHEMA
        || envelope.step > MAX_AGENT_STEPS
        || envelope.history_messages.len() > MAX_AGENT_CONTEXT_BYTES
        || envelope.tool_context.len() > MAX_AGENT_CONTEXT_BYTES
        || (envelope.step == 0) != envelope.observation.0.is_none()
        || envelope.observation().is_some_and(invalid_observation)
    {
        return Err(RuntimeClientError::InvalidFrame);
    }

    Ok(envelope)
}

/// Returns whether a tool observation violates the wire contract.
///
/// 约束检查点：
/// - 工具名称、`tool_call_id` 不能使用容易混淆的尾缀；
/// - status 仅允许 `ok` 与 `error`；
/// - content 超限与空白名都会触发拒绝，避免下游注入和误解析。
///
/// 参考实现：
/// - [modelcontextprotocol/servers pull #4480](https://github.com/modelcontextprotocol/servers/pull/4480)
/// - [CortexFS PR #89](https://github.com/LIghtJUNction/cortexfs/pull/89)
/// - [mark3labs/mcp-filesystem-server main branch](https://github.com/mark3labs/mcp-filesystem-server)
/// - [OpenAI Codex tool_call_id mismatch issue #8479](https://github.com/openai/codex/issues/8479)
/// - [modelcontextprotocol/servers pull #4523](https://github.com/modelcontextprotocol/servers/pull/4523)
/// - [modelcontextprotocol/python-sdk PR #1655](https://github.com/modelcontextprotocol/python-sdk/pull/1655)
fn invalid_observation(value: &AgentToolObservation) -> bool {
    !valid_name(value.tool_call_id())
        || !valid_name(value.name())
        || !matches!(value.status(), "ok" | "error")
        || value.content().len() > MAX_OBSERVATION_BYTES
}

/// Returns whether a tool-call identifier or tool name is canonical.
///
/// 该规则与 MCP 文件系统场景中的「工具名可解析、可追踪」目标一致。
/// - 禁止空名与过长名字。
/// - 首字符禁用特殊符号，避免与路径与命名空间语义混淆。
/// - 显式排除 `.sock` / `.d` 后缀，避免路径类标识冲突。
///
/// 相似逻辑：
/// - [rust-fs-mcp tool naming in response and catalog](https://docs.rs/crate/rust-fs-mcp/0.1.7/source/architecture.md#l556)
/// - [Model Context Protocol schema tooling naming fields](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-06-18/schema.ts)
/// - [OpenAI Codex tool call matching issue #2550](https://github.com/microsoft/autogen/issues/2550)
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name.strip_suffix(".sock").is_none()
        && name.strip_suffix(".d").is_none()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric()
                || (index != 0 && matches!(byte, b'.' | b'_' | b'+' | b'-'))
        })
}

#[cfg(test)]
mod tests;
