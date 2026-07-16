//! Narrow client and shared wire protocol for a runtime capability socket.
//!
//! The runtime-client module keeps wire framing and validation concerns isolated from
//! agent control policy, allowing protocol behavior to be audited as a stable ABI
//! boundary.
//!
//! 相关实现：
//! - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
//!   （帧类型匹配与兼容行为）
//! - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
//!   （`request_id` 相关约束）
//! - [rust-mcp-stack/rust-mcp-sdk PR #80](https://github.com/rust-mcp-stack/rust-mcp-sdk/pull/80)
//!   （newline framed 控制帧实践）

/// Agent-specific runtime-client helpers for creating and pinging child agents.
pub mod agent;

use serde::{Deserialize, Serialize};
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Maximum serialized response frame size for one socket read path, in bytes.
///
/// The bound avoids unbounded buffering if peers send oversized payloads or if
/// framing is malformed. It is intentionally strict to keep read-side memory and
/// parser behavior predictable.
const MAX_FRAME_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Runtime wire protocol errors returned by the runtime capability socket client.
///
/// The variants intentionally use stable, user-facing error buckets so protocol,
/// transport, and authorization failures can be handled without stringly-typed checks.
///
/// 相关实现：
/// - [modelcontextprotocol/servers #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
///   （错误码分类实践）
/// - [modelcontextprotocol/servers #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
///   （响应配对失败场景）
/// - [CortexFS PR #87](https://github.com/LIghtJUNction/cortexfs/pull/87)
pub enum RuntimeClientError {
    /// Request/response inputs are malformed or violate protocol constraints.
    InvalidEnvironment,
    /// Runtime control socket could not be reached.
    CannotConnect,
    /// Socket write failed while sending a control frame.
    CannotWrite,
    /// Socket read timed out or could not consume a response frame.
    CannotRead,
    /// Wire response is invalidly shaped or fails serde parsing.
    InvalidFrame,
    /// Runtime rejected request with a structured provider/runtime reason.
    Rejected(String),
}

/// Generates a legal request identifier from operating-system entropy.
///
/// The identifier is `{prefix}-{hex}` with 128-bit random data encoded as 32 hex chars.
/// The prefix and final length are validated before entropy is requested.
///
/// 相关实现：
/// - [rawr-ai/mcp-filesystem](https://github.com/rawr-ai/mcp-filesystem)
/// - [mark3labs/mcp-filesystem-server](https://github.com/mark3labs/mcp-filesystem-server)
/// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
pub fn fresh_request_id(prefix: &str) -> Result<String, RuntimeClientError> {
    if prefix.is_empty()
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || prefix.len().saturating_add(33) > 128
    {
        return Err(RuntimeClientError::InvalidEnvironment);
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_error| RuntimeClientError::CannotRead)?;
    let mut id = String::with_capacity(prefix.len() + 33);
    id.push_str(prefix);
    id.push('-');
    for byte in random {
        use std::fmt::Write as _;
        write!(id, "{byte:02x}").map_err(|_error| RuntimeClientError::CannotWrite)?;
    }
    Ok(id)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "op", deny_unknown_fields)]
/// Runtime control request frames sent over the capability socket.
///
/// Request frames are versionless in this module scope and must remain minimal to
/// preserve forward-compatibility between runtime and test fixtures.
///
/// 相关实现：
/// - [modelcontextprotocol/servers pull #4480](https://github.com/modelcontextprotocol/servers/pull/4480)
/// - [modelcontextprotocol/servers issue #1237](https://github.com/modelcontextprotocol/servers/issues/1237)
pub enum RequestFrame {
    #[serde(rename = "ping")]
    /// Heartbeat request used for startup validation and source reconciliation.
    ///
    /// This maps to `pong` responses in the control plane and is used to confirm
    /// agent/session/run identity before child execution.
    ///
    /// 相关实现：
    /// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
    /// - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
    Ping {
        /// Runtime authorization token.
        token: String,
        /// Request identifier used for response correlation.
        request_id: String,
        /// Agent identity that initiated the ping.
        agent: String,
        /// Session identity that initiated the ping.
        session: String,
        /// Run identifier for the current agent session lineage.
        run: String,
    },
    #[serde(rename = "agent.create")]
    /// Child-agent create request carrying identity, runtime command, and lifetime inputs.
    ///
    /// Mirrors `agent.create` flows consumed by the runtime socket server and tested
    /// by the same wire-level invariants used in `agent.create` issue/PR flows.
    ///
    /// 相关实现：
    /// - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
    /// - [CortexFS PR #89](https://github.com/LIghtJUNction/cortexfs/pull/89)
    CreateChild {
        /// Runtime authorization token.
        token: String,
        /// Request identifier used for response correlation.
        request_id: String,
        /// Parent agent name requesting child creation.
        agent: String,
        /// Parent session name requesting child creation.
        session: String,
        /// Parent run identifier for lineage tracking.
        run: String,
        /// Child object name allocated inside this control namespace.
        child: String,
        /// Child session namespace for the newly-created child.
        child_session: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Optional child filesystem path override.
        path: Option<String>,
        #[serde(
            default,
            deserialize_with = "deserialize_present_u32",
            skip_serializing_if = "Option::is_none"
        )]
        /// Optional PTY window hint in columns/rows semantics.
        window: Option<u32>,
        /// Text input payload for the child invocation.
        input: String,
        /// Requested child lifetime, e.g. `temp`, `owned`.
        life: String,
    },
}

/// Deserializes an optional `u32`, mapping a present value to `Some`.
/// Treats explicit JSON `window` values as numeric and preserves absence as `None`.
///
/// This helper enforces strict numeric parsing so malformed values are rejected
/// early instead of being silently defaulted.
///
/// 相关实现：MCP 客户端/服务端参数反序列化中对可选字段的显式区分。
/// - [rust-mcp-stack/rust-mcp-sdk](https://github.com/rust-mcp-stack/rust-mcp-sdk)
/// - [CortexFS PR #88](https://github.com/LIghtJUNction/cortexfs/pull/88)
fn deserialize_present_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    u32::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Result of a successful child-creation request.
///
/// 相关实现：
/// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
/// - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
pub struct CreateChildResult {
    /// Child object identifier allocated by the runtime.
    pub child: String,
    /// Child session identifier allocated by the runtime.
    pub child_session: String,
    /// Child process id of spawned child agent/runtime.
    pub pid: u32,
}

/// Runtime child-creation request fields assembled from environment or caller inputs.
///
/// Keeping this as a borrowed view avoids duplicate allocations when environment
/// reads already own references for the same logical request scope.
///
/// 相关实现：
/// - [modelcontextprotocol/servers issue #1237](https://github.com/modelcontextprotocol/servers/issues/1237)
/// - [CortexFS PR #88](https://github.com/LIghtJUNction/cortexfs/pull/88)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreateChildEnvironmentRequest<'a> {
    /// Request id supplied by caller-generated nonce space.
    pub request_id: &'a str,
    /// Child object name from the parent create command.
    pub child: &'a str,
    /// Child session name from the parent create command.
    pub child_session: &'a str,
    /// Optional child filesystem path override.
    pub path: Option<&'a str>,
    /// Optional PTY window hint, in columns/rows semantics.
    pub window: Option<u32>,
    /// User input text carried by the create request.
    pub input: &'a str,
    /// Requested life mode (`temp`, `owned`, etc.).
    pub life: &'a str,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Snapshot of the source root published by a successful ping.
/// The receipt is only emitted for successful ping handshakes.
///
/// 相关实现：
/// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
/// - [rawr-ai/mcp-filesystem](https://github.com/rawr-ai/mcp-filesystem)
pub struct RuntimeSourceReceipt {
    /// Absolute source path attached to the receipt.
    pub path: String,
    /// Source file-system device id.
    pub dev: u64,
    /// Source file-system inode number.
    pub ino: u64,
    /// Source kind marker used to gate ABI assumptions.
    pub kind: RuntimeSourceKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Stable source-kind families for receipt metadata.
///
/// 相关实现：
/// - [modelcontextprotocol/servers issue #4232](https://github.com/modelcontextprotocol/servers/issues/4232)
/// - [modelcontextprotocol/servers issue #4208](https://github.com/modelcontextprotocol/servers/issues/4208)
pub enum RuntimeSourceKind {
    /// Source directory is a plain directory.
    PlainDirectory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
/// Runtime control response frames produced by the capability host.
///
/// 响应体采用固定 `type` 进行解码分派，配套 `request_id` 做请求关联。
///
/// 相关实现：
/// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
/// - [modelcontextprotocol/servers issue #3404](https://github.com/modelcontextprotocol/servers/issues/3404)
pub enum ResponseFrame {
    #[serde(rename = "pong")]
    /// Startup/health response may carry source receipt.
    ///
    /// This pairs with `RequestFrame::Ping`.
    ///
    /// 相关实现：
    /// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
    /// - [modelcontextprotocol/go-sdk discussion #364](https://github.com/orgs/modelcontextprotocol/discussions/364)
    Pong {
        /// Request identifier from the matching ping request.
        request_id: String,
        /// Optional source receipt metadata if runtime returns it.
        receipt: Option<RuntimeSourceReceipt>,
    },
    #[serde(rename = "error")]
    /// Standardized protocol-level failure with errno-like code payload.
    ///
    /// This mirrors the control daemon error transport and feeds `RunCapabilityError`.
    ///
    /// 相关实现：
    /// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
    /// - [rust-mcp-stack/rust-mcp-sdk PR #87](https://github.com/rust-mcp-stack/rust-mcp-sdk/pull/87)
    Error {
        /// Request identifier from the matching request.
        request_id: String,
        /// Runtime errno-like rejection payload.
        errno: String,
    },
    #[serde(rename = "agent.created")]
    /// Success response for `agent.create`, returning concrete child identity metadata.
    ///
    /// This directly maps to `CreateChildResult`.
    ///
    /// 相关实现：
    /// - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
    /// - [CortexFS PR #89](https://github.com/LIghtJUNction/cortexfs/pull/89)
    ChildCreated {
        /// Request identifier from the matching request.
        request_id: String,
        /// Child creation result metadata.
        result: CreateChildResult,
    },
}

impl RequestFrame {
    #[must_use]
    /// Returns the request identifier carried by this frame.
    ///
    /// Used by `request` to detect response/request correlation across the socket.
    ///
    /// 相关实现：
    /// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
    /// - [MCP transport spec](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)
    pub fn request_id(&self) -> &str {
        match *self {
            Self::Ping { ref request_id, .. } | Self::CreateChild { ref request_id, .. } => {
                request_id
            }
        }
    }
}

/// Sends one wire frame, waits for one bounded response, and validates protocol pairing.
///
/// 该函数是本模块的关键边界：它写入单帧请求、读取单帧响应，并验证：
/// 1) response 类型与 request 的匹配性；
/// 2) `request_id` 的回环一致性；
/// 3) 帧尺寸和换行分隔边界。
///
/// 相关实现：
/// - [orgs/modelcontextprotocol discussions #364](https://github.com/orgs/modelcontextprotocol/discussions/364)
/// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
/// - [modelcontextprotocol/servers pull #80](https://github.com/modelcontextprotocol/servers/pull/80)
pub fn request(socket: &Path, frame: &RequestFrame) -> Result<ResponseFrame, RuntimeClientError> {
    let mut stream =
        UnixStream::connect(socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
    serde_json::to_writer(&mut stream, frame).map_err(|_error| RuntimeClientError::CannotWrite)?;
    stream
        .write_all(b"\n")
        .map_err(|_error| RuntimeClientError::CannotWrite)?;
    stream
        .set_read_timeout(Some(if cfg!(test) {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(5)
        }))
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take(MAX_FRAME_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_error| RuntimeClientError::CannotRead)?;
    if bytes.is_empty()
        || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FRAME_BYTES
        || bytes.last() != Some(&b'\n')
    {
        return Err(RuntimeClientError::InvalidFrame);
    }
    let response: ResponseFrame =
        serde_json::from_slice(&bytes).map_err(|_error| RuntimeClientError::InvalidFrame)?;
    let response_id = match response {
        ResponseFrame::Pong { ref request_id, .. }
        | ResponseFrame::Error { ref request_id, .. }
        | ResponseFrame::ChildCreated { ref request_id, .. } => request_id,
    };
    if response_id != frame.request_id()
        || !matches!(
            (frame, &response),
            (
                RequestFrame::Ping { .. },
                ResponseFrame::Pong { .. } | ResponseFrame::Error { .. }
            ) | (
                RequestFrame::CreateChild { .. },
                ResponseFrame::ChildCreated { .. } | ResponseFrame::Error { .. }
            )
        )
    {
        return Err(RuntimeClientError::InvalidFrame);
    }
    Ok(response)
}

/// Sends a ping frame to query source receipt and returns optional source metadata.
///
/// In startup handshakes this is treated as an optional capability check:
/// - `Ok(Some(_))` when runtime replies with source metadata.
/// - `Ok(None)` when protocol is intentionally not present.
/// - `Err(_)` for transport or protocol mismatch.
///
/// 相关讨论：
/// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
///   （路径/参数边界与兼容策略讨论）
/// - [modelcontextprotocol/servers issue #1237](https://github.com/modelcontextprotocol/servers/issues/1237)
///   （HTTP 响应与原始帧内容约束）
/// - [CortexFS PR #87](https://github.com/LIghtJUNction/cortexfs/pull/87)
pub fn ping(
    socket: &Path,
    token: &str,
    request_id: &str,
    agent: &str,
    session: &str,
    run: &str,
) -> Result<Option<RuntimeSourceReceipt>, RuntimeClientError> {
    match request(
        socket,
        &RequestFrame::Ping {
            token: token.to_owned(),
            request_id: request_id.to_owned(),
            agent: agent.to_owned(),
            session: session.to_owned(),
            run: run.to_owned(),
        },
    )? {
        ResponseFrame::Pong { receipt, .. } => Ok(receipt),
        ResponseFrame::Error { errno, .. } => Err(RuntimeClientError::Rejected(errno)),
        ResponseFrame::ChildCreated { .. } => Err(RuntimeClientError::InvalidFrame),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "wire helper keeps capability and parent identity fields explicit"
)]
/// Sends a child-agent create request through the control socket.
///
/// 调用前会先执行快速参数防御（如 `window == 0`），然后通过 `request` 做统一
/// 的帧写入、读边界与类型配对校验。
///
/// 相关实现：
/// - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
/// - [CortexFS PR #88](https://github.com/LIghtJUNction/cortexfs/pull/88)
/// - [modelcontextprotocol/servers pull #4480](https://github.com/modelcontextprotocol/servers/pull/4480)
pub fn create_child(
    socket: &Path,
    token: &str,
    request_id: &str,
    agent: &str,
    session: &str,
    run: &str,
    child: &str,
    child_session: &str,
    path: Option<&str>,
    window: Option<u32>,
    input: &str,
    life: &str,
) -> Result<CreateChildResult, RuntimeClientError> {
    if window == Some(0) {
        return Err(RuntimeClientError::InvalidEnvironment);
    }
    match request(
        socket,
        &RequestFrame::CreateChild {
            token: token.to_owned(),
            request_id: request_id.to_owned(),
            agent: agent.to_owned(),
            session: session.to_owned(),
            run: run.to_owned(),
            child: child.to_owned(),
            child_session: child_session.to_owned(),
            path: path.map(str::to_owned),
            window,
            input: input.to_owned(),
            life: life.to_owned(),
        },
    )? {
        ResponseFrame::ChildCreated { result, .. } => Ok(result),
        ResponseFrame::Error { errno, .. } => Err(RuntimeClientError::Rejected(errno)),
        ResponseFrame::Pong { .. } => Err(RuntimeClientError::InvalidFrame),
    }
}

/// Builds a child-create request from environment variables and dispatches it to the control socket.
///
/// This helper mirrors legacy env-driven invocations and fails closed when any required
/// variable is missing or malformed.
///
/// 相关安全讨论：
/// - [CortexFS PR #87](https://github.com/LIghtJUNction/cortexfs/pull/87)
/// - [modelcontextprotocol/servers pull #4480](https://github.com/modelcontextprotocol/servers/pull/4480)
/// - [mark3labs/mcp-filesystem-server](https://github.com/mark3labs/mcp-filesystem-server)
/// - [rawr-ai/mcp-filesystem](https://github.com/rawr-ai/mcp-filesystem)
pub fn create_child_from_environment(
    request: CreateChildEnvironmentRequest<'_>,
) -> Result<CreateChildResult, RuntimeClientError> {
    if request.window == Some(0) {
        return Err(RuntimeClientError::InvalidEnvironment);
    }
    let socket = env::var_os("CTX_CONTROL_SOCKET").ok_or(RuntimeClientError::InvalidEnvironment)?;
    let token =
        env::var("CTX_CONTROL_TOKEN").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let agent = env::var("CTX_AGENT").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let session =
        env::var("CTX_SESSION").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    let run = env::var("CTX_RUN_ID").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
    create_child(
        &PathBuf::from(socket),
        &token,
        request.request_id,
        &agent,
        &session,
        &run,
        request.child,
        request.child_session,
        request.path,
        request.window,
        request.input,
        request.life,
    )
}

/// Performs optional startup ping from caller environment variables.
///
/// If both `CTX_CONTROL_SOCKET` and `CTX_CONTROL_TOKEN` are present, this performs
/// a startup compatibility ping; if neither is present, it becomes no-op.
/// Mixed presence of exactly one variable fails closed.
///
/// 相关实现：
/// - [modelcontextprotocol/servers issue #4206](https://github.com/modelcontextprotocol/servers/issues/4206)
/// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
/// - [CortexFS PR #89](https://github.com/LIghtJUNction/cortexfs/pull/89)
pub fn ping_from_environment(
    agent: &str,
) -> Result<Option<RuntimeSourceReceipt>, RuntimeClientError> {
    let socket = env::var_os("CTX_CONTROL_SOCKET");
    let token = env::var("CTX_CONTROL_TOKEN").ok();
    match (socket, token) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(RuntimeClientError::InvalidEnvironment),
        (Some(socket), Some(token)) => {
            let session =
                env::var("CTX_SESSION").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
            let run =
                env::var("CTX_RUN_ID").map_err(|_error| RuntimeClientError::InvalidEnvironment)?;
            ping(
                &PathBuf::from(socket),
                &token,
                &format!("startup-{run}"),
                agent,
                &session,
                &run,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::process::Command;
    use std::thread;

    /// Builds a stable ping-request fixture used by most request-path tests.
    ///
    /// 参考：
    /// - [modelcontextprotocol/servers issue #4207](https://github.com/modelcontextprotocol/servers/issues/4207)
    /// - [modelcontextprotocol/servers issue #1770](https://github.com/modelcontextprotocol/servers/issues/1770)
    fn ping_frame() -> RequestFrame {
        RequestFrame::Ping {
            token: "token".to_owned(),
            request_id: "request-1".to_owned(),
            agent: "agent".to_owned(),
            session: "session".to_owned(),
            run: "run".to_owned(),
        }
    }

    /// 启动临时 Unix socket 并返回一次 `request()` 的完整解析结果。
    ///
    /// 该 helper 把 peer 侧响应固定成单帧输入，便于对照 request/response 配对逻辑。
    ///
    /// 参考：
    /// - [modelcontextprotocol/go-sdk discussion #364](https://github.com/orgs/modelcontextprotocol/discussions/364)
    /// - [modelcontextprotocol/servers pull #80](https://github.com/modelcontextprotocol/servers/pull/80)
    fn response(bytes: Vec<u8>) -> Result<ResponseFrame, RuntimeClientError> {
        let root = tempfile::tempdir().map_err(|_error| RuntimeClientError::CannotConnect)?;
        let socket = root.path().join("control.sock");
        let listener =
            UnixListener::bind(&socket).map_err(|_error| RuntimeClientError::CannotConnect)?;
        let server = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request_bytes = Vec::new();
            let _ignored = BufReader::new(&mut stream).read_until(b'\n', &mut request_bytes);
            let _ignored = stream.write_all(&bytes);
        });
        let result = request(&socket, &ping_frame());
        let _ignored = server.join();
        result
    }

    /// 校验响应 id 与响应类型错配时返回 `InvalidFrame`，防止“看起来成功但语义不对”。
    #[test]
    fn rejects_wrong_id_and_response_type() {
        assert_eq!(
            response(b"{\"type\":\"pong\",\"request_id\":\"wrong\"}\n".to_vec()),
            Err(RuntimeClientError::InvalidFrame)
        );
        assert_eq!(response(b"{\"type\":\"agent.created\",\"request_id\":\"request-1\",\"result\":{\"child\":\"c\",\"child_session\":\"s\",\"pid\":1}}\n".to_vec()), Err(RuntimeClientError::InvalidFrame));
    }

    /// 校验带 source receipt 的 ping 响应解析，确保 `kind` 与 `receipt` 原样透传。
    #[test]
    fn ping_returns_authoritative_source_receipt() {
        let response = response(b"{\"type\":\"pong\",\"request_id\":\"request-1\",\"receipt\":{\"path\":\"/source\",\"dev\":7,\"ino\":9,\"kind\":\"plain-directory\"}}\n".to_vec());
        assert_eq!(
            response,
            Ok(ResponseFrame::Pong {
                request_id: "request-1".to_owned(),
                receipt: Some(RuntimeSourceReceipt {
                    path: "/source".to_owned(),
                    dev: 7,
                    ino: 9,
                    kind: RuntimeSourceKind::PlainDirectory
                })
            })
        );
    }

    /// 验证 `fresh_request_id` 约束字符集、长度、前缀及不稳定前缀拒绝逻辑。
    #[test]
    fn fresh_request_ids_are_legal_and_distinct() -> Result<(), RuntimeClientError> {
        let first = fresh_request_id("tsh-cache")?;
        let second = fresh_request_id("tsh-cache")?;
        assert_ne!(first, second);
        assert!(first.len() <= 128);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        );
        assert!(fresh_request_id("bad prefix").is_err());
        Ok(())
    }

    /// 拒绝超长响应与缺失 newline 的 framing，保持 `MAX_FRAME_BYTES` 与 `read_until` 语义严格。
    #[test]
    fn rejects_oversized_and_missing_newline() {
        let mut oversized = vec![b'x'; usize::try_from(MAX_FRAME_BYTES).unwrap_or(16_384)];
        oversized.push(b'\n');
        assert_eq!(response(oversized), Err(RuntimeClientError::InvalidFrame));
        assert_eq!(
            response(br#"{"type":"pong","request_id":"request-1"}"#.to_vec()),
            Err(RuntimeClientError::InvalidFrame)
        );
    }

    /// 通过不回包场景验证 `read_timeout_is_bounded` 在 test/非 test 分支都不漏读阻塞。
    #[test]
    fn read_timeout_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let socket = root.path().join("control.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                thread::sleep(Duration::from_millis(200));
            }
        });
        assert_eq!(
            request(&socket, &ping_frame()),
            Err(RuntimeClientError::CannotRead)
        );
        let _ignored = server.join();
        Ok(())
    }

    /// 校验 `create_child` 的请求-响应一一对应，尤其 `life/path` 关键字段映射。
    #[test]
    fn create_child_response_has_exact_parity() {
        let root = tempfile::tempdir().ok();
        assert!(root.is_some());
        let Some(root) = root else { return };
        let socket = root.path().join("control.sock");
        let listener = UnixListener::bind(&socket).ok();
        assert!(listener.is_some());
        let Some(listener) = listener else { return };
        let server = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut bytes = Vec::new();
            let _ignored = BufReader::new(&mut stream).read_until(b'\n', &mut bytes);
            let frame = serde_json::from_slice::<RequestFrame>(&bytes);
            assert!(matches!(
                frame,
                Ok(RequestFrame::CreateChild { life, path, .. })
                    if life == "temp" && path.as_deref() == Some("/ctx/home/1000/tool")
            ));
            let _ignored = stream.write_all(b"{\"type\":\"agent.created\",\"request_id\":\"request-1\",\"result\":{\"child\":\"c\",\"child_session\":\"s\",\"pid\":42}}\n");
        });
        let result = create_child(
            &socket,
            "token",
            "request-1",
            "agent",
            "session",
            "run",
            "c",
            "s",
            Some("/ctx/home/1000/tool"),
            Some(2048),
            "input",
            "temp",
        );
        assert!(server.join().is_ok());
        assert_eq!(
            result,
            Ok(CreateChildResult {
                child: "c".to_owned(),
                child_session: "s".to_owned(),
                pid: 42
            })
        );
    }

    /// 严格检查 `window` 的可选数值语义，缺省、显式与非法值三种边界都应被约束。
    #[test]
    fn create_child_window_wire_is_numeric_optional_and_strict()
    -> Result<(), Box<dyn std::error::Error>> {
        let frame = RequestFrame::CreateChild {
            token: "token".to_owned(),
            request_id: "request-1".to_owned(),
            agent: "agent".to_owned(),
            session: "session".to_owned(),
            run: "run".to_owned(),
            child: "child".to_owned(),
            child_session: "child-session".to_owned(),
            path: None,
            window: Some(2048),
            input: "work".to_owned(),
            life: "owned".to_owned(),
        };
        let encoded = serde_json::to_value(&frame)?;
        assert_eq!(
            encoded.get("window").and_then(serde_json::Value::as_u64),
            Some(2048)
        );
        let mut absent = encoded;
        if let Some(object) = absent.as_object_mut() {
            object.remove("window");
        }
        assert!(matches!(
            serde_json::from_value::<RequestFrame>(absent),
            Ok(RequestFrame::CreateChild { window: None, .. })
        ));
        let mut omitted = frame.clone();
        if let RequestFrame::CreateChild { ref mut window, .. } = omitted {
            *window = None;
        }
        assert!(serde_json::to_value(omitted)?.get("window").is_none());
        for value in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("1"),
            serde_json::Value::Null,
            serde_json::json!(4_294_967_296_u64),
        ] {
            let mut invalid = serde_json::to_value(&frame)?;
            invalid
                .as_object_mut()
                .ok_or("serialized create child request should be an object")?
                .insert("window".to_owned(), value);
            assert!(serde_json::from_value::<RequestFrame>(invalid).is_err());
        }
        Ok(())
    }

    /// 以零窗口作为非法输入，验证在连接前快速失败，避免下游 socket 错误泄漏语义。
    #[test]
    fn zero_window_fails_before_connect() {
        assert_eq!(
            create_child(
                Path::new("/definitely/missing.sock"),
                "token",
                "request-1",
                "agent",
                "session",
                "run",
                "child",
                "child-session",
                None,
                Some(0),
                "work",
                "owned",
            ),
            Err(RuntimeClientError::InvalidEnvironment)
        );
    }

    /// 在局部子进程里复现部分环境变量缺失分支，保持与主进程调用一致的闭环行为。
    #[test]
    #[ignore = "subprocess entrypoint for environment isolation"]
    fn partial_environment_subprocess() {
        assert_eq!(
            ping_from_environment("agent"),
            Err(RuntimeClientError::InvalidEnvironment)
        );
    }

    /// 子进程级别复用 `partial_environment_subprocess`，避免环境脏状态污染主测试进程。
    #[test]
    fn partial_environment_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new(env::current_exe()?)
            .arg("--exact")
            .arg("tests::partial_environment_subprocess")
            .arg("--ignored")
            .env("CTX_CONTROL_TOKEN", "partial")
            .env_remove("CTX_CONTROL_SOCKET")
            .status()?;
        assert!(status.success());
        Ok(())
    }
}
