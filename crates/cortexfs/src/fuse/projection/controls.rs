use super::*;

pub(crate) fn model_alias_name(abi_path: &str) -> Option<&str> {
    let alias = abi_path.strip_prefix("model/")?;
    is_model_alias(alias).then_some(alias)
}

pub(crate) fn model_control_dir_entries() -> Vec<FuseDirEntry> {
    MODEL_CONTROL_FILES
        .iter()
        .map(|file| FuseDirEntry::new((*file).to_owned(), FuseFileType::Regular))
        .collect()
}

pub(crate) fn validate_model_control_write(abi_path: &str, content: &str) -> Result<(), FuseError> {
    match parse_abi_path(abi_path).model_control_file() {
        Some("cap") if inspect_model_capabilities(content).is_ok() => Ok(()),
        Some("driver") if parse_model_driver_routes(content).is_ok() => Ok(()),
        Some("effort") if ModelEffort::parse(content).is_some() => Ok(()),
        Some("session") if matches!(content.trim(), "none" | "socket") => Ok(()),
        Some("cap" | "driver" | "effort" | "session") => Err(FuseError::InvalidContent),
        _ if !content.contains('\0') => Ok(()),
        _ => Err(FuseError::InvalidContent),
    }
}

pub(crate) fn projected_regular_file(
    abi_path: &str,
    content: String,
    mode: u32,
) -> Result<ProjectedFile, FuseError> {
    Ok(ProjectedFile {
        attr: FuseAttr::new(
            abi_path.to_owned(),
            FuseFileType::Regular,
            u64::try_from(content.len()).map_err(|_error| FuseError::Io)?,
            mode,
        ),
        content: Some(content),
    })
}

pub(crate) fn projected_metadata_mode(
    abi_path: &str,
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<u32, FuseError> {
    let mode = metadata.permissions().mode();
    if abi_path.is_empty() && metadata.is_dir() {
        return Ok((mode & !0o7777) | 0o755);
    }
    if abi_path == "agent" && metadata.is_dir() {
        return Ok((mode & !0o7777) | 0o1777);
    }
    if FuseProjection::agent_control_target(abi_path).is_some_and(|(_, file)| file == "perm") {
        let content = support::plain::read_small_text_file(path, MAX_FUSE_SMALL_READ_BYTES)
            .map_err(|_error| FuseError::Io)?;
        let permissions =
            AgentPermissions::parse_control(&content).ok_or(FuseError::InvalidContent)?;
        return Ok((mode & !0o7777) | permissions.mode());
    }
    Ok(mode)
}
