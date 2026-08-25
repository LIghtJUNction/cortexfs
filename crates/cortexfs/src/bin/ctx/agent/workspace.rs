use crate::*;
use std::{env, fs, os::unix::fs::MetadataExt, path::Path};

const WORKSPACE_TARGET: &str = "/workspace";
const WORKSPACE_OPTIONS: &str = "rbind,nosuid,nodev";

/// Adds the exact current directory to the selected agent's mount policy.
///
/// Bare `ctx` uses this; other agent starts require a pre-existing policy mount.
pub(crate) fn ensure_default_workspace_mount(
    root: &Path,
    agent: &str,
    workspace: &Path,
) -> Result<(), CliError> {
    let workspace = absolute_existing_path(workspace).map_err(|error| {
        CliError::unavailable(format!(
            "cannot resolve workspace {}: {error}",
            workspace.display()
        ))
    })?;
    let metadata = fs::metadata(&workspace).map_err(|error| {
        CliError::unavailable(format!(
            "cannot inspect workspace {}: {error}",
            workspace.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(CliError::usage(
            "automatic workspace mount requires a directory",
        ));
    }
    let view = derive_agent_runtime_view(root, agent).map_err(|error| {
        CliError::unavailable(format!(
            "cannot derive agent runtime view for {agent}: {error:?}"
        ))
    })?;
    if metadata.uid() != view.identity().uid() {
        return Err(CliError::usage(
            "automatic workspace mount requires the current directory to be agent-owned",
        ));
    }
    let home = env::var_os("HOME").and_then(|path| absolute_existing_path(Path::new(&path)).ok());
    if home.as_deref() == Some(workspace.as_path()) {
        return Err(CliError::usage(
            "automatic workspace mount refuses the whole home directory; use an explicit mount",
        ));
    }
    let source = agent_source_root(root);
    let mount_path = cortexfs_paths::agent_control_file_path(&source, agent, "mount");
    let content = fs::read_to_string(&mount_path).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", mount_path.display()))
    })?;
    let table = MountTable::parse(&content)
        .map_err(|error| CliError::unavailable(format!("invalid agent mount policy: {error:?}")))?;
    let source_text = workspace.display().to_string();
    if let Some(entry) = table
        .entries()
        .iter()
        .find(|entry| entry.source() == source_text && entry.target() == WORKSPACE_TARGET)
    {
        return match entry.mode() {
            cortexfs::MountMode::ReadWrite => Ok(()),
            cortexfs::MountMode::ReadOnly => Err(CliError::usage(
                "workspace mount policy is read-only; change it explicitly before using bare ctx",
            )),
        };
    }

    let line = format!("{source_text}\t{WORKSPACE_TARGET}\trw\t{WORKSPACE_OPTIONS}");
    let updated = append_mount_line(&content, &line);
    atomic_replace_text_preserving_metadata(&mount_path, &updated).map_err(|error| {
        CliError::unavailable(format!("cannot update workspace mount policy: {error}"))
    })
}
fn append_mount_line(content: &str, line: &str) -> String {
    let mut updated = content.trim_end_matches('\n').to_owned();
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(line);
    updated.push('\n');
    updated
}

#[cfg(test)]
mod tests {
    use super::{append_mount_line, ensure_default_workspace_mount};
    use std::{fs, io};

    #[test]
    fn append_mount_line_preserves_existing_entries() {
        assert_eq!(
            append_mount_line("/ctx\t/ctx\tro\trbind\n", "/work\t/workspace\trw\trbind"),
            "/ctx\t/ctx\tro\trbind\n/work\t/workspace\trw\trbind\n"
        );
    }

    #[test]
    fn automatic_workspace_mount_adds_exact_owned_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        crate::ensure_reference_tree(root.path())
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        let providers = root.path().join("providers.d");
        let cache = root.path().join("provider-models");
        fs::create_dir_all(&providers)?;
        fs::create_dir_all(&cache)?;
        cortexfs::reference::bootstrap::ensure_runtime_models_from(root.path(), &providers, &cache)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;

        ensure_default_workspace_mount(root.path(), "executor", &workspace)
            .map_err(|error| io::Error::other(error.message))?;

        let mount = fs::read_to_string(root.path().join("agent").join("executor.d").join("mount"))?;
        assert!(mount.contains("/workspace\trw\trbind,nosuid,nodev"));
        Ok(())
    }
}
