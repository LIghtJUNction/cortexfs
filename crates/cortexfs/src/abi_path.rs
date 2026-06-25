#![allow(
    clippy::redundant_pub_crate,
    reason = "private split module exposes parser helpers to the parent crate"
)]

use std::borrow::Cow;

use crate::{
    AgentControlKind, ContextJsonlKind, DEFAULT_MODEL_ALIAS, HELPER_MODEL_ALIAS,
    MAX_OBJECT_NAME_LEN, ROOT_ENTRIES, SessionControlKind, SessionIndexKind,
};

pub use crate::abi_path_parse::{classify_abi_path, parse_abi_path};

/// Stable executable object classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectClass {
    /// Pure inference endpoint.
    Model,
    /// Policy-bound orchestrator endpoint.
    Agent,
    /// Executable capability endpoint.
    Tool,
}

impl ObjectClass {
    /// Parses a stable object class name.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "model" => Some(Self::Model),
            "agent" => Some(Self::Agent),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }

    /// Returns the ABI directory name for this object class.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Agent => "agent",
            Self::Tool => "tool",
        }
    }

    /// Returns the stable `ctx file` type for an executable object.
    #[must_use]
    pub const fn exec_type(self) -> &'static str {
        match self {
            Self::Model => "ctx.model.exec",
            Self::Agent => "ctx.agent.exec",
            Self::Tool => "ctx.tool.exec",
        }
    }

    /// Returns the stable `ctx file` type for an object socket.
    #[must_use]
    pub const fn socket_type(self) -> &'static str {
        match self {
            Self::Model => "ctx.model.socket",
            Self::Agent => "ctx.agent.socket",
            Self::Tool => "ctx.tool.socket",
        }
    }

    /// Returns the stable `ctx file` type for an object control path.
    #[must_use]
    pub const fn control_type(self) -> &'static str {
        match self {
            Self::Model => "ctx.model.control",
            Self::Agent => "ctx.agent.control",
            Self::Tool => "ctx.tool.control",
        }
    }
}

/// Returns whether a top-level path component is part of the public ABI.
#[must_use]
pub fn is_root_entry(name: &str) -> bool {
    ROOT_ENTRIES.contains(&name)
}

/// Returns whether a model, agent, tool, session, or shared object name is valid.
#[must_use]
pub fn is_object_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_OBJECT_NAME_LEN {
        return false;
    }
    if name.strip_suffix(".sock").is_some() || name.strip_suffix(".d").is_some() {
        return false;
    }

    name.bytes().enumerate().all(|(index, byte)| {
        let is_extra = matches!(byte, b'.' | b'_' | b'+' | b'-');
        let valid = byte.is_ascii_alphanumeric() || is_extra;
        if index == 0 {
            byte.is_ascii_alphanumeric()
        } else {
            valid
        }
    })
}

/// Returns whether a model name uses the provider/model namespace shape.
#[must_use]
pub fn is_model_name(name: &str) -> bool {
    let Some((provider, model)) = name.split_once('/') else {
        return false;
    };
    !model.contains('/') && is_object_name(provider) && is_object_name(model)
}

pub(crate) fn is_model_reference(name: &str) -> bool {
    is_model_name(name) || matches!(name, DEFAULT_MODEL_ALIAS | HELPER_MODEL_ALIAS)
}

pub(crate) fn is_object_name_for_class(class: ObjectClass, name: &str) -> bool {
    match class {
        ObjectClass::Model => is_model_name(name),
        ObjectClass::Agent | ObjectClass::Tool => is_object_name(name),
    }
}

/// Parsed `CortexFS` ABI path shape.
///
/// This is the typed companion to the stable `ctx.*` strings returned by
/// [`classify_abi_path`]. It is intended for internal routing and validation;
/// the filesystem ABI remains the path shape and stable type strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiPathKind<'a> {
    /// Path does not match any stable v1 ABI shape.
    Unknown,
    /// `model/<provider>`.
    ModelDir { provider: &'a str },
    /// `model/<provider>/<model>`, `agent/<name>`, or `tool/<name>`.
    ObjectExec {
        class: ObjectClass,
        provider: Option<&'a str>,
        name: &'a str,
    },
    /// `model/<provider>/<model>.sock`, `agent/<name>.sock`, or `tool/<name>.sock`.
    ObjectSocket {
        class: ObjectClass,
        provider: Option<&'a str>,
        name: &'a str,
    },
    /// Object control file path under `<name>.d/`.
    ObjectControl {
        class: ObjectClass,
        provider: Option<&'a str>,
        name: &'a str,
        file: &'a str,
    },
    /// `home/<uid>` and valid descendants not otherwise classified.
    HomeDir,
    /// Durable session root, for example `home/<uid>/agent/<agent>/session`.
    SessionRoot,
    /// Durable session instance directory.
    SessionDir { session: &'a str },
    /// Direct durable session file.
    SessionFile { session: &'a str, file: &'a str },
    /// Reserved durable session index file.
    SessionIndex { kind: SessionIndexKind },
    /// Durable file below `context/` in a session.
    SessionContextFile {
        session: &'a str,
        first: &'a str,
        second: Option<&'a str>,
    },
    /// `shared/<space>` and valid descendants not otherwise classified.
    SharedDir { space: &'a str },
    /// `shared/<space>/tool/<tool>`.
    SharedToolExec { space: &'a str, name: &'a str },
    /// `shared/<space>/tool/<tool>.d/<file>`.
    SharedToolControl {
        space: &'a str,
        name: &'a str,
        file: &'a str,
    },
    /// `shared/<space>/queue`.
    SharedQueueRoot { space: &'a str },
    /// A fixed child directory below `shared/<space>/queue`.
    SharedQueueDir { space: &'a str, name: &'a str },
    /// `shared/<space>/result`.
    SharedResult { space: &'a str },
    /// A syntactically valid ordinary file under an ABI-owned subtree.
    Ordinary,
}

impl AbiPathKind<'_> {
    /// Returns the stable `ctx file type` string for this parsed path.
    #[must_use]
    pub fn stable_type(self) -> &'static str {
        match self {
            Self::Unknown => "ctx.unknown",
            Self::ModelDir { .. } => "ctx.model.dir",
            Self::ObjectExec { class, .. } => class.exec_type(),
            Self::ObjectSocket { class, .. } => class.socket_type(),
            Self::ObjectControl { class, .. } => class.control_type(),
            Self::HomeDir => "ctx.home.dir",
            Self::SessionRoot | Self::SessionDir { .. } => "ctx.session.dir",
            Self::SessionFile {
                file: "messages.jsonl",
                ..
            } => "ctx.session.messages",
            Self::SessionFile {
                file: "events.jsonl",
                ..
            } => "ctx.session.events",
            Self::SessionFile { .. }
            | Self::SessionIndex { .. }
            | Self::SessionContextFile { .. }
            | Self::Ordinary => "ctx.ordinary",
            Self::SharedDir { .. } => "ctx.shared.dir",
            Self::SharedToolExec { .. } => "ctx.shared.tool.exec",
            Self::SharedToolControl { .. } => "ctx.shared.tool.control",
            Self::SharedQueueRoot { .. } | Self::SharedQueueDir { .. } => "ctx.shared.queue",
            Self::SharedResult { .. } => "ctx.shared.result",
        }
    }
}

impl<'a> AbiPathKind<'a> {
    /// Returns an executable object class and stable name, when this path is an executable object.
    #[must_use]
    pub fn executable_object(self) -> Option<(ObjectClass, Cow<'a, str>)> {
        match self {
            Self::ObjectExec {
                class: ObjectClass::Model,
                provider: Some(provider),
                name,
            } => Some((ObjectClass::Model, Cow::Owned(format!("{provider}/{name}")))),
            Self::ObjectExec { class, name, .. } => Some((class, Cow::Borrowed(name))),
            _ => None,
        }
    }

    /// Returns a model control-file name for `model/<provider>/<model>.d/<file>`.
    #[must_use]
    pub const fn model_control_file(self) -> Option<&'a str> {
        match self {
            Self::ObjectControl {
                class: ObjectClass::Model,
                file,
                ..
            } => Some(file),
            _ => None,
        }
    }

    /// Returns a global or shared tool schema path.
    #[must_use]
    pub fn is_tool_schema(self) -> bool {
        matches!(
            self,
            Self::ObjectControl {
                class: ObjectClass::Tool,
                file: "schema",
                ..
            } | Self::SharedToolControl { file: "schema", .. }
        )
    }

    /// Returns a control file name for object `.d/` paths that carry policy or mount syntax.
    #[must_use]
    pub const fn control_file(self) -> Option<&'a str> {
        match self {
            Self::ObjectControl { file, .. } | Self::SharedToolControl { file, .. } => Some(file),
            _ => None,
        }
    }

    /// Returns the fixed agent-control kind, if this is `agent/<name>.d/<file>`.
    #[must_use]
    pub fn agent_control_kind(self) -> Option<AgentControlKind> {
        match self {
            Self::ObjectControl {
                class: ObjectClass::Agent,
                file,
                ..
            } => AgentControlKind::parse(file),
            _ => None,
        }
    }

    /// Returns the fixed session-index kind for reserved `session/index/*` files.
    #[must_use]
    pub fn session_index_kind(self) -> Option<SessionIndexKind> {
        match self {
            Self::SessionIndex { kind } => Some(kind),
            _ => None,
        }
    }

    /// Returns the fixed session control kind for direct session control files.
    #[must_use]
    pub fn session_control_kind(self) -> Option<SessionControlKind> {
        match self {
            Self::SessionFile { session, file } if is_object_name(session) => {
                SessionControlKind::parse(file)
            }
            _ => None,
        }
    }

    /// Returns whether this path is a durable session instance directory.
    #[must_use]
    pub fn is_session_instance(self) -> bool {
        matches!(self, Self::SessionDir { session } if is_object_name(session))
    }

    /// Returns a stable context JSONL kind for session `context/*` files.
    #[must_use]
    pub fn context_jsonl_kind(self) -> Option<ContextJsonlKind> {
        match self {
            Self::SessionContextFile {
                session,
                first,
                second,
            } if is_object_name(session) => match (first, second) {
                ("facts.jsonl", None) => Some(ContextJsonlKind::Facts),
                ("decisions.jsonl", None) => Some(ContextJsonlKind::Decisions),
                ("refs.jsonl", None) => Some(ContextJsonlKind::Refs),
                ("swap", Some("index.jsonl")) => Some(ContextJsonlKind::SwapIndex),
                ("dedup", Some("index.jsonl")) => Some(ContextJsonlKind::DedupIndex),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns whether this path is `context/pack.json` below a durable session.
    #[must_use]
    pub fn is_context_pack(self) -> bool {
        matches!(
            self,
            Self::SessionContextFile {
                session,
                first: "pack.json",
                second: None,
            } if is_object_name(session)
        )
    }
}
