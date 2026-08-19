use super::*;

const CHANNEL_TOOL_SCHEMA: &str = r#"{"type":"object","additionalProperties":true}"#;

/// Materializes every catalogued ZeroClaw-compatible channel namespace.
pub(crate) fn ensure_reference_channels(root: &Path) -> Result<(), ReferenceTreeError> {
    for spec in cortexfs_channels::CHANNEL_CATALOG {
        let channel = spec.id;
        create_reference_dir(&cortexfs_paths::channel_path(root, channel))?;
        create_reference_dir(&cortexfs_paths::channel_tool_path(root, channel))?;
        create_reference_dir(&cortexfs_paths::channel_control_path(root, channel))?;
        for (file, content) in [
            ("id", format!("{channel}\n")),
            (
                "driver",
                format!("{}\n", cortexfs_channels::CHANNEL_SOCKET_ABI),
            ),
            ("cap", "tool.*\n".to_owned()),
            ("status", "unavailable\n".to_owned()),
            ("health", "unknown\n".to_owned()),
        ] {
            ensure_channel_file(
                &cortexfs_paths::channel_control_file_path(root, channel, file),
                &content,
            )?;
        }
        for tool in spec.tool_names() {
            ensure_channel_tool(root, channel, &tool)?;
        }
    }
    Ok(())
}

fn ensure_channel_tool(root: &Path, channel: &str, name: &str) -> Result<(), ReferenceTreeError> {
    let path = cortexfs_paths::channel_tool_path(root, channel).join(name);
    let control = path.with_file_name(format!("{name}.d"));
    create_reference_dir(&control)?;
    let policy = channel_tool_policy(name);
    for (file, content) in [
        ("id", format!("{name}\n")),
        (
            "description",
            format!("{channel} channel capability tool: {name}\n"),
        ),
        ("schema", format!("{CHANNEL_TOOL_SCHEMA}\n")),
        ("cap", format!("{name}\n")),
        ("policy", policy),
    ] {
        ensure_channel_file(&control.join(file), &content)?;
    }
    ensure_channel_file(
        &path,
        "#!/bin/sh\nexec /usr/bin/cortexfs-channel-tool \"$@\"\n",
    )?;
    set_reference_executable(&path)
}

fn channel_tool_policy(name: &str) -> String {
    ["architect_t", "coder_t", "reviewer_t", "worker_t"]
        .iter()
        .map(|subject| format!("allow {subject} tool:{name} execute"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn ensure_channel_file(path: &Path, content: &str) -> Result<(), ReferenceTreeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_reference_text(path, content)
        }
        Ok(_) | Err(_) => Err(ReferenceTreeError::CannotCreate),
    }
}
