fn model_alias_name(abi_path: &str) -> Option<&str> {
    let alias = abi_path.strip_prefix("model/")?;
    matches!(alias, DEFAULT_MODEL_ALIAS | HELPER_MODEL_ALIAS).then_some(alias)
}

fn model_control_dir_entries() -> Vec<FuseV1DirEntry> {
    MODEL_CONTROL_FILES
        .iter()
        .map(|file| FuseV1DirEntry::new((*file).to_owned(), FuseV1FileType::Regular))
        .collect()
}

fn validate_model_control_write(abi_path: &str, content: &str) -> Result<(), FuseV1Error> {
    match parse_abi_path(abi_path).model_control_file() {
        Some("cap") if inspect_model_capabilities(content).is_ok() => Ok(()),
        Some("driver") if parse_model_driver_routes(content).is_ok() => Ok(()),
        Some("effort") if ModelEffort::parse(content).is_some() => Ok(()),
        Some("fallback") if parse_model_fallback(content).1.is_ok() => Ok(()),
        Some("session") if matches!(content.trim(), "none" | "socket") => Ok(()),
        Some("cap" | "driver" | "effort" | "fallback" | "session") => {
            Err(FuseV1Error::InvalidContent)
        }
        _ if !content.contains('\0') => Ok(()),
        _ => Err(FuseV1Error::InvalidContent),
    }
}

fn virtual_regular_entry(
    content: &str,
    mode: u32,
) -> Result<Option<(FuseV1FileType, u64, u32)>, FuseV1Error> {
    Ok(Some((
        FuseV1FileType::Regular,
        u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
        mode,
    )))
}

fn projected_metadata_mode(abi_path: &str, metadata: &fs::Metadata) -> u32 {
    let mode = metadata.permissions().mode();
    if abi_path.is_empty() && metadata.is_dir() {
        return (mode & !0o7777) | 0o755;
    }
    mode
}
