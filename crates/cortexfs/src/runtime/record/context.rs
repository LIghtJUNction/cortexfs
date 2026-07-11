use super::*;

pub(crate) fn require_parent_session_context(
    parent_session_dir: &Path,
) -> Result<(), ChildContextRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !is_plain_existing_file(&parent_session_dir.join(file)) {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    if !is_plain_existing_dir(&parent_session_dir.join("context")) {
        return Err(ChildContextRecordError::MissingParentSession);
    }
    Ok(())
}

pub(crate) fn require_child_context_files(child_dir: &Path) -> Result<(), ChildContextRecordError> {
    for file in CHILD_RESULT_REQUIRED_FILES {
        if !is_plain_existing_file(&child_dir.join(file)) {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        if !is_plain_existing_dir(&child_dir.join(dir)) {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    Ok(())
}

pub(crate) fn require_agent_schedule_parent_context(
    parent_session_dir: &Path,
) -> Result<(), AgentScheduleRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !is_plain_existing_file(&parent_session_dir.join(file)) {
            return Err(AgentScheduleRecordError::MissingParentSession);
        }
    }
    if !is_plain_existing_dir(&parent_session_dir.join("context")) {
        return Err(AgentScheduleRecordError::MissingParentSession);
    }
    Ok(())
}

pub(crate) fn agent_schedule_child_record_error(
    error: ChildContextRecordError,
) -> AgentScheduleRecordError {
    match error {
        ChildContextRecordError::MissingParentSession => {
            AgentScheduleRecordError::MissingParentSession
        }
        ChildContextRecordError::CannotRecord
        | ChildContextRecordError::InvalidChildName
        | ChildContextRecordError::InvalidAgentName
        | ChildContextRecordError::InvalidSessionName
        | ChildContextRecordError::InvalidStatus
        | ChildContextRecordError::InvalidText
        | ChildContextRecordError::InvalidRefs => AgentScheduleRecordError::CannotRecord,
    }
}

pub(crate) fn schedule_child_handoff_materialized(
    parent_session_dir: &Path,
    handoff: &AgentScheduleChildHandoff,
) -> Result<bool, AgentScheduleRecordError> {
    schedule_child_context_matches(
        parent_session_dir,
        handoff.child(),
        handoff.agent(),
        handoff.session(),
        handoff.handoff(),
    )
}

pub(crate) fn schedule_child_context_matches(
    parent_session_dir: &Path,
    child_name: &str,
    child_agent: &str,
    child_session: &str,
    handoff: &str,
) -> Result<bool, AgentScheduleRecordError> {
    let child_dir = parent_session_dir
        .join("context")
        .join("child")
        .join(child_name);
    match child_dir.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() => {
            require_agent_schedule_child_context_files(&child_dir)?;
            if read_child_schedule_file(&child_dir, "agent")? != format!("{child_agent}\n")
                || read_child_schedule_file(&child_dir, "session")? != format!("{child_session}\n")
                || read_child_schedule_file(&child_dir, "handoff.md")?
                    != ensure_trailing_newline(handoff)
            {
                return Err(AgentScheduleRecordError::CannotRecord);
            }
            let _status = read_child_schedule_status(&child_dir)?;
            let refs_jsonl = read_child_schedule_file(&child_dir, "refs.jsonl")?;
            if !inspect_context_jsonl(ContextJsonlKind::Refs, &refs_jsonl).is_ok() {
                return Err(AgentScheduleRecordError::CannotRecord);
            }
            Ok(true)
        }
        Ok(_metadata) => Err(AgentScheduleRecordError::CannotRecord),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_error) => Err(AgentScheduleRecordError::CannotRecord),
    }
}

pub(crate) fn require_agent_schedule_child_context_files(
    child_dir: &Path,
) -> Result<(), AgentScheduleRecordError> {
    for file in CHILD_RESULT_REQUIRED_FILES {
        if !is_plain_existing_file(&child_dir.join(file)) {
            return Err(AgentScheduleRecordError::CannotRecord);
        }
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        if !is_plain_existing_dir(&child_dir.join(dir)) {
            return Err(AgentScheduleRecordError::CannotRecord);
        }
    }
    Ok(())
}

pub(crate) fn read_child_schedule_file(
    child_dir: &Path,
    file: &str,
) -> Result<String, AgentScheduleRecordError> {
    fs::read_to_string(child_dir.join(file))
        .map_err(|_error| AgentScheduleRecordError::CannotRecord)
}

pub(crate) fn read_child_schedule_status(
    child_dir: &Path,
) -> Result<Option<ChildContextStatus>, AgentScheduleRecordError> {
    let child_metadata = match child_dir.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err(AgentScheduleRecordError::CannotRecord),
    };
    if !child_metadata.is_dir() {
        return Err(AgentScheduleRecordError::CannotRecord);
    }
    let status_path = child_dir.join("status");
    let metadata = match status_path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err(AgentScheduleRecordError::CannotRecord),
    };
    if !metadata.is_file() {
        return Err(AgentScheduleRecordError::CannotRecord);
    }
    let status =
        fs::read_to_string(status_path).map_err(|_error| AgentScheduleRecordError::CannotRecord)?;
    let status = status.trim();
    ChildContextStatus::parse(status)
        .map_or(Err(AgentScheduleRecordError::CannotRecord), |status| {
            Ok(Some(status))
        })
}

pub(crate) fn write_child_context_file(
    child_dir: &Path,
    file: &str,
    content: &str,
) -> Result<(), ChildContextRecordError> {
    atomic_replace_text(&child_dir.join(file), content)
        .map_err(|_error| ChildContextRecordError::CannotRecord)
}
