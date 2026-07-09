use crate::{
    AbiPathKind, MODEL_ROUTE_FILE, ObjectClass, SessionIndexKind, is_model_name, is_object_name,
};

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

pub(crate) fn parse_simple_object_path<'a>(
    class: ObjectClass,
    parts: &[&'a str],
) -> AbiPathKind<'a> {
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

pub(crate) fn parse_model_object_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((provider, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };
    if !is_object_name(provider) {
        return AbiPathKind::Unknown;
    }
    if *provider == MODEL_ROUTE_FILE && rest.is_empty() {
        return AbiPathKind::ModelRoute;
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

pub(crate) fn parse_home_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
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

pub(crate) fn parse_home_agent_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
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

pub(crate) fn parse_home_model_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
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

pub(crate) fn parse_shared_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
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

pub(crate) fn parse_shared_queue_child<'a>(space: &'a str, rest: &[&'a str]) -> AbiPathKind<'a> {
    let Some((name, tail)) = rest.split_first() else {
        return AbiPathKind::SharedQueueRoot { space };
    };
    if tail.is_empty() && is_shared_queue_entry(name) {
        AbiPathKind::SharedQueueDir { space, name }
    } else {
        AbiPathKind::Ordinary
    }
}

pub(crate) fn parse_shared_tool_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
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

pub(crate) fn parse_shared_agent_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
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

pub(crate) fn parse_shared_model_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
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

pub(crate) fn parse_session_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
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
        } else if tail.len() == 1 && tail.first().is_some_and(|key| is_object_name(key)) {
            match *first {
                "by-cwd" => AbiPathKind::SessionIndex {
                    kind: SessionIndexKind::ByCwd,
                },
                "by-hash" => AbiPathKind::SessionIndex {
                    kind: SessionIndexKind::ByHash,
                },
                "by-uuid" => AbiPathKind::SessionIndex {
                    kind: SessionIndexKind::ByUuid,
                },
                _ => AbiPathKind::Ordinary,
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

pub(crate) fn parse_session_context_path<'a>(
    session: &'a str,
    tail: &[&'a str],
) -> AbiPathKind<'a> {
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
