use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::{SocketDebugTiming, write_socket_frame};
use crate::SocketRuntimeError;

/// Parses a JSON frame and returns timing metadata only for valid debug frames.
///
/// 功能：识别带有 `debug: true` 的帧并创建阶段计时上下文。
///
/// 类似实现：line-based 调试埋点常见于进程 socket 协议场景。
/// 相关讨论：
/// - MCP 运行时诊断与超时处理思路
///   [https://github.com/modelcontextprotocol/servers/pull/4479](https://github.com/modelcontextprotocol/servers/pull/4479)
///   [https://github.com/modelcontextprotocol/servers/pull/4208](https://github.com/modelcontextprotocol/servers/pull/4208)
/// - MCP request-id 与异常恢复可观测性：[#3404](https://github.com/modelcontextprotocol/servers/issues/3404)
/// - JSON 消息健壮性与长度保护案例：[#4207](https://github.com/modelcontextprotocol/servers/issues/4207)、[#4206](https://github.com/modelcontextprotocol/servers/issues/4206)
pub(in crate::runtime::socket) fn socket_debug_timing_from_frame(
    frame: &str,
) -> Option<SocketDebugTiming> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    if value.get("debug").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    Some(SocketDebugTiming {
        start_unix_ms: current_unix_millis(),
        request_start_unix_ms: None,
    })
}

/// Returns the current unix timestamp in milliseconds since UNIX epoch.
/// Used by timing frames to keep elapsed durations monotonic at frame level.
pub(super) fn current_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

/// Emits a debug timing frame with elapsed milliseconds for the given stage.
///
/// 功能：将阶段耗时按 JSONL 帧写回 socket，供上层审计与可视化。
/// 相关项目：
/// - [modelcontextprotocol/servers](https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem)
/// - [mark3labs/mcp-filesystem-server](https://github.com/mark3labs/mcp-filesystem-server)
/// - [rust-mcp-stack/rust-mcp-sdk issue discussion #141](https://github.com/rust-mcp-stack/rust-mcp-sdk/issues/141)
/// - MCP JSON 解析/可恢复性案例：[#4206](https://github.com/modelcontextprotocol/servers/issues/4206)
fn write_socket_debug_timing_frame(
    stream: &mut std::os::unix::net::UnixStream,
    timing: SocketDebugTiming,
    stage: &str,
) -> Result<(), SocketRuntimeError> {
    let elapsed_ms = current_unix_millis().saturating_sub(timing.start_unix_ms);
    let mut frame = serde_json::json!({
        "type": "debug",
        "stage": stage,
        "elapsed_ms": elapsed_ms
    });
    if let Some(request_start_unix_ms) = timing.request_start_unix_ms
        && let Some(object) = frame.as_object_mut()
    {
        object.insert(
            "request_elapsed_ms".to_owned(),
            serde_json::json!(current_unix_millis().saturating_sub(request_start_unix_ms)),
        );
    }
    write_socket_frame(stream, &frame.to_string())
}

/// Writes an optional debug timing frame when a timing context exists.
/// Keeps timing emission behind `write_socket_frame` to match existing socket
/// framing semantics.
///
/// 相关 PR：
/// - MCP filesystem server fixes around debug/transport handling [#4411](https://github.com/modelcontextprotocol/servers/pull/4411)
/// - MCP transport 观测与 request-id 关联讨论 [#3404](https://github.com/modelcontextprotocol/servers/issues/3404)
pub(in crate::runtime::socket) fn write_optional_socket_debug_timing_frame(
    stream: &mut std::os::unix::net::UnixStream,
    timing: Option<SocketDebugTiming>,
    stage: &str,
) -> Result<(), SocketRuntimeError> {
    if let Some(timing) = timing {
        write_socket_debug_timing_frame(stream, timing, stage)?;
    }
    Ok(())
}

/// Applies debug timing settings from environment variables when enabled.
/// Mirrors process-environment integration points used by `ctxagent` launcher.
///
/// 相关 PR：
/// - project PR [#89](https://github.com/LIghtJUNction/cortexfs/pull/89)
/// - MCP message 可恢复性与异常隔离讨论：[#4206](https://github.com/modelcontextprotocol/servers/issues/4206)
pub(in crate::runtime::socket) fn apply_socket_debug_timing_env(
    command: &mut Command,
    timing: Option<SocketDebugTiming>,
) {
    if let Some(timing) = timing {
        command.env("CTX_AGENT_DEBUG_TIMING", "1").env(
            "CTX_AGENT_DEBUG_START_UNIX_MS",
            timing.start_unix_ms.to_string(),
        );
    }
}

/// Returns true when a frame has debug timing event shape.
///
/// 参考：
/// - JSONL 调试消息与阶段粒度上报惯例：
///   - [modelcontextprotocol/servers #4479](https://github.com/modelcontextprotocol/servers/pull/4479)
///   - [modelcontextprotocol/servers #4232](https://github.com/modelcontextprotocol/servers/issues/4232)
pub(in crate::runtime::socket) fn is_socket_debug_timing_frame(
    frame: &str,
    timing: Option<SocketDebugTiming>,
) -> bool {
    if timing.is_none() {
        return false;
    }
    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.len() != 3
        || value.get("type").and_then(Value::as_str) != Some("debug")
        || value.get("elapsed_ms").and_then(Value::as_u64).is_none()
    {
        return false;
    }
    matches!(
        value.get("stage").and_then(Value::as_str),
        Some("agent_runner_ready" | "model_spawn_start" | "model_spawned" | "first_model_frame")
    )
}
