use crate::*;

use serde::Deserialize;
use serde_json::Value;

use crate::{MAX_SOCKET_FRAME_BYTES, is_stable_chroot_absolute_path, validate_socket_object_field};

/// Stable socket session scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketSessionScope {
    /// Private to the current Linux uid and resumable.
    Private,
    /// Stored in a shared space when allowed by policy and mount visibility.
    Shared,
    /// Process-local session that need not survive socket close or agent exit.
    Temp,
}

impl SocketSessionScope {
    /// Parses a stable socket session scope.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "private" => Some(Self::Private),
            "shared" => Some(Self::Shared),
            "temp" => Some(Self::Temp),
            _ => None,
        }
    }

    /// Returns the stable scope word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
            Self::Temp => "temp",
        }
    }
}

/// Canonical JSONL socket request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SocketRequest {
    /// Start or continue a run by sending user input into a session.
    Send {
        /// Client idempotency id.
        id: String,
        /// `CortexFS` session name. Defaults to `default` when omitted.
        session: String,
        /// Session storage scope. Defaults to `private` when omitted.
        scope: SocketSessionScope,
        /// Optional cwd inside the agent chroot.
        cwd: Option<String>,
        /// Optional host workspace directory bound at `/workspace` for this run.
        workspace: Option<String>,
        /// User input text.
        input: String,
    },
    /// Execute one tsh command through the authoritative agent runtime.
    Tsh {
        id: String,
        session: String,
        args: Vec<String>,
    },
    /// Resume a session stream, optionally after an event id.
    Resume {
        /// `CortexFS` session name. Defaults to `default` when omitted.
        session: String,
        /// Optional event id cursor.
        after: Option<String>,
    },
    /// Cancel a run.
    Cancel {
        /// Run id to cancel.
        id: String,
    },
    /// Stop an agent and its owned descendants through its authoritative runtime.
    Stop {
        /// Agent name; must match the socket runtime identity.
        agent: String,
    },
    /// Health check.
    Ping,
}

/// Stable socket request parse or validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SocketRequestError {
    FrameTooLarge { bytes: usize },
    EmptyFrame,
    MultipleFrames,
    InvalidJson,
    RequestNotObject,
    MissingOp,
    UnknownOp(String),
    MissingStringField(&'static str),
    InvalidField { field: &'static str, value: String },
}

impl SocketRequestError {
    /// Returns a stable errno name for this parse failure.
    #[must_use]
    pub const fn errno(&self) -> &'static str {
        match *self {
            Self::FrameTooLarge { .. } => "EMSGSIZE",
            Self::EmptyFrame
            | Self::MultipleFrames
            | Self::InvalidJson
            | Self::RequestNotObject
            | Self::MissingOp
            | Self::UnknownOp(_)
            | Self::MissingStringField(_)
            | Self::InvalidField { .. } => "EINVAL",
        }
    }
}

/// Parses one v1 JSONL socket request frame.
///
/// Unknown fields are ignored by design. Only the stable fields that affect
/// `CortexFS` session semantics are consumed.
pub fn parse_socket_request_frame(frame: &str) -> Result<SocketRequest, SocketRequestError> {
    if frame.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(SocketRequestError::FrameTooLarge { bytes: frame.len() });
    }

    let frame = trim_jsonl_frame(frame)?;
    if !frame.trim_start().starts_with('{') {
        return Err(SocketRequestError::RequestNotObject);
    }
    serde_json::from_str::<Value>(frame).map_err(|_error| SocketRequestError::InvalidJson)?;
    let request = serde_path_to_error::deserialize::<_, SocketRequestFrame>(
        &mut serde_json::Deserializer::from_str(frame),
    )
    .map_err(|error| socket_request_deserialize_error(&error, frame))?;
    request.try_into()
}

pub(crate) fn trim_jsonl_frame(frame: &str) -> Result<&str, SocketRequestError> {
    let trimmed = frame.trim_end_matches(['\r', '\n']);
    if trimmed.trim().is_empty() {
        return Err(SocketRequestError::EmptyFrame);
    }
    if trimmed.lines().count() > 1 {
        return Err(SocketRequestError::MultipleFrames);
    }
    Ok(trimmed)
}

#[derive(Deserialize)]
#[serde(tag = "op")]
enum SocketRequestFrame {
    #[serde(rename = "send")]
    Send {
        id: String,
        #[serde(default = "default_socket_session")]
        session: String,
        #[serde(default)]
        scope: SocketSessionScopeFrame,
        cwd: Option<String>,
        workspace: Option<String>,
        input: String,
    },
    #[serde(rename = "tsh")]
    Tsh {
        id: String,
        #[serde(default = "default_socket_session")]
        session: String,
        args: Vec<String>,
    },
    #[serde(rename = "resume")]
    Resume {
        #[serde(default = "default_socket_session")]
        session: String,
        after: Option<String>,
    },
    #[serde(rename = "cancel")]
    Cancel { id: String },
    #[serde(rename = "stop")]
    Stop { agent: String },
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SocketSessionScopeFrame {
    #[default]
    Private,
    Shared,
    Temp,
}

impl From<SocketSessionScopeFrame> for SocketSessionScope {
    fn from(scope: SocketSessionScopeFrame) -> Self {
        match scope {
            SocketSessionScopeFrame::Private => Self::Private,
            SocketSessionScopeFrame::Shared => Self::Shared,
            SocketSessionScopeFrame::Temp => Self::Temp,
        }
    }
}

impl TryFrom<SocketRequestFrame> for SocketRequest {
    type Error = SocketRequestError;

    fn try_from(request: SocketRequestFrame) -> Result<Self, Self::Error> {
        match request {
            SocketRequestFrame::Send {
                id,
                session,
                scope,
                cwd,
                workspace,
                input,
            } => parse_socket_send_request(id, session, scope.into(), cwd, workspace, input),
            SocketRequestFrame::Tsh { id, session, args } => {
                validate_socket_object_field("id", &id)?;
                validate_socket_object_field("session", &session)?;
                if args.is_empty()
                    || args.len() > 64
                    || args
                        .iter()
                        .any(|arg| arg.contains('\0') || arg.len() > 4096)
                {
                    return Err(SocketRequestError::InvalidField {
                        field: "args",
                        value: args.join(" "),
                    });
                }
                Ok(Self::Tsh { id, session, args })
            }
            SocketRequestFrame::Resume { session, after } => {
                parse_socket_resume_request(session, after)
            }
            SocketRequestFrame::Cancel { id } => parse_socket_cancel_request(id),
            SocketRequestFrame::Stop { agent } => {
                if is_object_name(&agent) {
                    Ok(Self::Stop { agent })
                } else {
                    Err(SocketRequestError::InvalidField {
                        field: "agent",
                        value: agent,
                    })
                }
            }
            SocketRequestFrame::Ping => Ok(Self::Ping),
        }
    }
}

pub(crate) fn parse_socket_send_request(
    id: String,
    session: String,
    scope: SocketSessionScope,
    cwd: Option<String>,
    workspace: Option<String>,
    input: String,
) -> Result<SocketRequest, SocketRequestError> {
    validate_socket_object_field("id", &id)?;
    validate_socket_object_field("session", &session)?;
    validate_optional_socket_cwd(cwd.as_deref())?;
    validate_optional_socket_workspace(workspace.as_deref())?;
    if input.contains('\0') {
        return Err(SocketRequestError::InvalidField {
            field: "input",
            value: input,
        });
    }

    Ok(SocketRequest::Send {
        id,
        session,
        scope,
        cwd,
        workspace,
        input,
    })
}

pub(crate) fn parse_socket_resume_request(
    session: String,
    after: Option<String>,
) -> Result<SocketRequest, SocketRequestError> {
    validate_socket_object_field("session", &session)?;
    validate_optional_socket_object_field("after", after.as_deref())?;
    Ok(SocketRequest::Resume { session, after })
}

pub(crate) fn parse_socket_cancel_request(id: String) -> Result<SocketRequest, SocketRequestError> {
    validate_socket_object_field("id", &id)?;
    Ok(SocketRequest::Cancel { id })
}

pub(crate) fn default_socket_session() -> String {
    "default".to_owned()
}

pub(crate) fn validate_optional_socket_cwd(cwd: Option<&str>) -> Result<(), SocketRequestError> {
    if let Some(cwd) = cwd
        && !is_stable_chroot_absolute_path(cwd)
    {
        return Err(SocketRequestError::InvalidField {
            field: "cwd",
            value: cwd.to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn validate_optional_socket_workspace(
    workspace: Option<&str>,
) -> Result<(), SocketRequestError> {
    let Some(workspace) = workspace else {
        return Ok(());
    };
    if support::path::is_absolute_host_workspace_path(workspace) {
        Ok(())
    } else {
        Err(SocketRequestError::InvalidField {
            field: "workspace",
            value: workspace.to_owned(),
        })
    }
}

pub(crate) fn validate_optional_socket_object_field(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), SocketRequestError> {
    if let Some(value) = value {
        validate_socket_object_field(field, value)?;
    }
    Ok(())
}

pub(crate) fn socket_request_deserialize_error(
    error: &serde_path_to_error::Error<serde_json::Error>,
    frame: &str,
) -> SocketRequestError {
    if error.inner().is_syntax() || error.inner().is_eof() {
        return SocketRequestError::InvalidJson;
    }
    if let Some(error) = socket_request_stable_field_error(frame) {
        return error;
    }
    let message = error.inner().to_string();
    if message.contains("missing field `op`") {
        return SocketRequestError::MissingOp;
    }
    if message.contains("unknown variant")
        && message.contains("private")
        && message.contains("shared")
        && message.contains("temp")
    {
        return socket_request_scope_error(message);
    }
    match error.path().to_string().as_str() {
        "." => SocketRequestError::RequestNotObject,
        "op" => socket_request_unknown_op_error(&message).unwrap_or(SocketRequestError::MissingOp),
        "id" => SocketRequestError::MissingStringField("id"),
        "session" => SocketRequestError::MissingStringField("session"),
        "scope" => socket_request_scope_error(message),
        "cwd" => SocketRequestError::MissingStringField("cwd"),
        "after" => SocketRequestError::MissingStringField("after"),
        "input" => SocketRequestError::MissingStringField("input"),
        _ => SocketRequestError::InvalidJson,
    }
}

pub(crate) fn socket_request_stable_field_error(frame: &str) -> Option<SocketRequestError> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    let object = value.as_object()?;
    let op = object.get("op")?.as_str()?;
    match op {
        "send" => socket_string_field_error(object, "id")
            .or_else(|| socket_string_field_error(object, "session"))
            .or_else(|| socket_string_field_error(object, "scope"))
            .or_else(|| socket_scope_value_error(object))
            .or_else(|| socket_string_field_error(object, "cwd"))
            .or_else(|| socket_string_field_error(object, "input")),
        "resume" => socket_string_field_error(object, "session")
            .or_else(|| socket_string_field_error(object, "after")),
        "cancel" => socket_string_field_error(object, "id"),
        _ => None,
    }
}

pub(crate) fn socket_string_field_error(
    object: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Option<SocketRequestError> {
    object
        .get(field)
        .filter(|value| !value.is_string())
        .map(|_value| SocketRequestError::MissingStringField(field))
}

pub(crate) fn socket_scope_value_error(
    object: &serde_json::Map<String, Value>,
) -> Option<SocketRequestError> {
    let scope = object.get("scope")?.as_str()?;
    SocketSessionScope::parse(scope)
        .is_none()
        .then(|| SocketRequestError::InvalidField {
            field: "scope",
            value: scope.to_owned(),
        })
}

pub(crate) fn socket_request_scope_error(message: String) -> SocketRequestError {
    let value = quoted_json_error_value(&message).unwrap_or(message);
    SocketRequestError::InvalidField {
        field: "scope",
        value,
    }
}

pub(crate) fn socket_request_unknown_op_error(message: &str) -> Option<SocketRequestError> {
    if !message.contains("unknown variant") {
        return None;
    }
    let value = quoted_json_error_value(message)?;
    Some(SocketRequestError::UnknownOp(value))
}

pub(crate) fn quoted_json_error_value(message: &str) -> Option<String> {
    let start = message.find('`')? + 1;
    let end = message.get(start..)?.find('`')? + start;
    message.get(start..end).map(ToOwned::to_owned)
}
