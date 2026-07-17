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
        Some("fallback") if parse_model_fallback(content).1.is_ok() => Ok(()),
        Some("session") if matches!(content.trim(), "none" | "socket") => Ok(()),
        Some("cap" | "driver" | "effort" | "fallback" | "session") => {
            Err(FuseError::InvalidContent)
        }
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

pub(crate) fn projected_metadata_mode(abi_path: &str, metadata: &fs::Metadata) -> u32 {
    let mode = metadata.permissions().mode();
    if abi_path.is_empty() && metadata.is_dir() {
        return (mode & !0o7777) | 0o755;
    }
    if abi_path == "agent" && metadata.is_dir() {
        return (mode & !0o7777) | 0o1777;
    }
    mode
}
