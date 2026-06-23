#![allow(
    clippy::redundant_pub_crate,
    reason = "private split module exposes parser helpers to the parent crate"
)]

use std::borrow::Cow;

use crate::{
    AgentControlKind, ContextJsonlKind, MAX_OBJECT_NAME_LEN, ROOT_ENTRIES, SessionControlKind,
    SessionIndexKind,
};

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
    /// Returns the stable `ctx file classify` string for this parsed path.
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

/// Classifies a relative `CortexFS` ABI path by path shape.
#[must_use]
pub fn classify_abi_path(path: &str) -> &'static str {
    parse_abi_path(path).stable_type()
}

/// Parses a relative `CortexFS` ABI path by path shape.
#[must_use]
pub fn parse_abi_path(path: &str) -> AbiPathKind<'_> {
    let trimmed = path.strip_prefix("./").map_or(path, |value| value);

    if trimmed.is_empty() || trimmed.split('/').any(str::is_empty) {
        return AbiPathKind::Unknown;
    }

    let parts = trimmed.split('/').collect::<Vec<_>>();
    let Some((first, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };
    match *first {
        "model" => parse_model_object_path(rest),
        "agent" => parse_simple_object_path(ObjectClass::Agent, rest),
        "tool" => parse_simple_object_path(ObjectClass::Tool, rest),
        "home" => parse_home_path(rest),
        "shared" => parse_shared_path(rest),
        _ => AbiPathKind::Unknown,
    }
}

fn parse_simple_object_path<'a>(class: ObjectClass, parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((name, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };

    if let Some(object_name) = name.strip_suffix(".sock") {
        return if rest.is_empty() && is_object_name(object_name) {
            AbiPathKind::ObjectSocket {
                class,
                provider: None,
                name: object_name,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    if let Some(object_name) = name.strip_suffix(".d") {
        return if let Some((file, _remaining)) = rest.split_first()
            && is_object_name(object_name)
        {
            AbiPathKind::ObjectControl {
                class,
                provider: None,
                name: object_name,
                file,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    if rest.is_empty() && is_object_name(name) {
        AbiPathKind::ObjectExec {
            class,
            provider: None,
            name,
        }
    } else {
        AbiPathKind::Unknown
    }
}

fn parse_model_object_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((provider, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };
    if !is_object_name(provider) {
        return AbiPathKind::Unknown;
    }
    let Some((name, rest)) = rest.split_first() else {
        return AbiPathKind::ModelDir { provider };
    };

    if let Some(object_name) = name.strip_suffix(".sock") {
        return if rest.is_empty() && is_model_name(&format!("{provider}/{object_name}")) {
            AbiPathKind::ObjectSocket {
                class: ObjectClass::Model,
                provider: Some(provider),
                name: object_name,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    if let Some(object_name) = name.strip_suffix(".d") {
        return if let Some((file, _remaining)) = rest.split_first()
            && is_model_name(&format!("{provider}/{object_name}"))
        {
            AbiPathKind::ObjectControl {
                class: ObjectClass::Model,
                provider: Some(provider),
                name: object_name,
                file,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    let model_name = format!("{provider}/{name}");
    if rest.is_empty() && is_model_name(&model_name) {
        AbiPathKind::ObjectExec {
            class: ObjectClass::Model,
            provider: Some(provider),
            name,
        }
    } else {
        AbiPathKind::Unknown
    }
}

fn parse_home_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((_uid, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };

    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::HomeDir;
    };
    match *first {
        "agent" => parse_home_agent_path(rest),
        "model" => parse_home_model_path(rest),
        _ => AbiPathKind::HomeDir,
    }
}

fn parse_home_agent_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((agent, rest)) = parts.split_first() else {
        return AbiPathKind::HomeDir;
    };
    if !is_object_name(agent) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::HomeDir;
    };
    match *first {
        "session" => parse_session_path(rest),
        _ => AbiPathKind::HomeDir,
    }
}

fn parse_home_model_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((provider, rest)) = parts.split_first() else {
        return AbiPathKind::HomeDir;
    };
    let Some((model_dir, rest)) = rest.split_first() else {
        return AbiPathKind::HomeDir;
    };
    let Some(model) = model_dir.strip_suffix(".d") else {
        return AbiPathKind::HomeDir;
    };
    if !is_model_name(&format!("{provider}/{model}")) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::HomeDir;
    };
    match *first {
        "session" => parse_session_path(rest),
        _ => AbiPathKind::HomeDir,
    }
}

fn parse_shared_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((space, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };
    if !is_object_name(space) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    match *first {
        "agent" => parse_shared_agent_path(space, rest),
        "model" => parse_shared_model_path(space, rest),
        "tool" => parse_shared_tool_path(space, rest),
        "queue" if rest.is_empty() => AbiPathKind::SharedQueueRoot { space },
        "queue" => parse_shared_queue_child(space, rest),
        "result" if rest.is_empty() => AbiPathKind::SharedResult { space },
        "result" => AbiPathKind::Ordinary,
        _ => AbiPathKind::SharedDir { space },
    }
}

fn parse_shared_queue_child<'a>(space: &'a str, rest: &[&'a str]) -> AbiPathKind<'a> {
    let Some((name, tail)) = rest.split_first() else {
        return AbiPathKind::SharedQueueRoot { space };
    };
    if tail.is_empty() && is_shared_queue_entry(name) {
        AbiPathKind::SharedQueueDir { space, name }
    } else {
        AbiPathKind::Ordinary
    }
}

fn parse_shared_tool_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((name, rest)) = parts.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    if let Some(tool_name) = name.strip_suffix(".d") {
        return if let Some((file, _remaining)) = rest.split_first()
            && is_object_name(tool_name)
        {
            AbiPathKind::SharedToolControl {
                space,
                name: tool_name,
                file,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    if rest.is_empty() && is_object_name(name) {
        AbiPathKind::SharedToolExec { space, name }
    } else {
        AbiPathKind::Unknown
    }
}

fn parse_shared_agent_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((agent, rest)) = parts.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    if !is_object_name(agent) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    match *first {
        "session" => parse_session_path(rest),
        _ => AbiPathKind::SharedDir { space },
    }
}

fn parse_shared_model_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((provider, rest)) = parts.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    let Some((model_dir, rest)) = rest.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    let Some(model) = model_dir.strip_suffix(".d") else {
        return AbiPathKind::SharedDir { space };
    };
    if !is_model_name(&format!("{provider}/{model}")) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    match *first {
        "session" => parse_session_path(rest),
        _ => AbiPathKind::SharedDir { space },
    }
}

fn parse_session_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((session, rest)) = parts.split_first() else {
        return AbiPathKind::SessionRoot;
    };
    if !is_object_name(session) {
        return AbiPathKind::Unknown;
    }

    let Some((first, tail)) = rest.split_first() else {
        return AbiPathKind::SessionDir { session };
    };
    if *session == "index" {
        return if tail.is_empty() && *first == "list" {
            AbiPathKind::SessionIndex {
                kind: SessionIndexKind::List,
            }
        } else if tail.is_empty() && *first == "current" {
            AbiPathKind::SessionIndex {
                kind: SessionIndexKind::Current,
            }
        } else if *first == "by-cwd"
            && tail.len() == 1
            && tail.first().is_some_and(|hash| !hash.is_empty())
        {
            AbiPathKind::SessionIndex {
                kind: SessionIndexKind::ByCwd,
            }
        } else {
            AbiPathKind::Ordinary
        };
    }
    if *first == "context" {
        return parse_session_context_path(session, tail);
    }
    if tail.is_empty() {
        AbiPathKind::SessionFile {
            session,
            file: first,
        }
    } else {
        AbiPathKind::Ordinary
    }
}

fn parse_session_context_path<'a>(session: &'a str, tail: &[&'a str]) -> AbiPathKind<'a> {
    let Some((first, rest)) = tail.split_first() else {
        return AbiPathKind::Ordinary;
    };
    AbiPathKind::SessionContextFile {
        session,
        first,
        second: rest.first().copied(),
    }
}

const fn is_shared_queue_entry(name: &str) -> bool {
    matches!(
        name.as_bytes(),
        b"inbox" | b"pending" | b"lease" | b"claimed" | b"done" | b"failed"
    )
}
