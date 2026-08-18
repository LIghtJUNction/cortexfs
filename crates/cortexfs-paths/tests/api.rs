#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn public_paths_keep_roles_distinct() {
        assert_eq!(
            cortexfs_paths::agent_client_socket("coder"),
            PathBuf::from("/ctx/agent/coder.sock")
        );
        assert_eq!(
            cortexfs_paths::system_agent_runtime_socket("coder"),
            PathBuf::from("/run/cortexfs/agent/coder.sock")
        );
        assert_eq!(
            cortexfs_paths::agent_backing_socket(Path::new("/storage/current"), "coder"),
            PathBuf::from("/storage/current/agent/coder.sock")
        );
    }

    #[test]
    fn root_paths_are_composable_for_external_clients() {
        let root = Path::new("/ctx");
        assert_eq!(
            cortexfs_paths::agent_path(root, "coder"),
            root.join("agent/coder")
        );
        assert_eq!(
            cortexfs_paths::agent_session_path(root, "1000", "coder", "default"),
            root.join("home/1000/agent/coder/session/default")
        );
        assert_eq!(
            cortexfs_paths::channel_config_path("discord"),
            PathBuf::from("/etc/cortexfs/channels/discord.toml")
        );
        assert_eq!(
            cortexfs_paths::root_entry_path(root, "shared"),
            Some(PathBuf::from("/ctx/shared"))
        );
        assert_eq!(
            cortexfs_paths::model_control_file_path(root, "openai", "gpt-5.6", "limit"),
            PathBuf::from("/ctx/model/openai/gpt-5.6.d/limit")
        );
        assert_eq!(
            cortexfs_paths::session_index_file_path(
                Path::new("/ctx/home/1000/agent/coder/session/default"),
                "current",
            ),
            PathBuf::from("/ctx/home/1000/agent/coder/session/default/index/current")
        );
    }

    #[test]
    fn host_and_runtime_paths_keep_private_roles_explicit() {
        assert_eq!(
            cortexfs_paths::storage_generation_path(Path::new("/var/lib/cortexfs/storage"), "v1"),
            PathBuf::from("/var/lib/cortexfs/storage/generations/v1")
        );
        assert_eq!(
            cortexfs_paths::provider_secret_path("openai"),
            PathBuf::from("/var/lib/cortexfs/secrets/provider/openai")
        );
        assert_eq!(
            cortexfs_paths::user_agent_runtime_socket(
                Path::new("/run/user/1000"),
                "scope",
                "coder",
            ),
            PathBuf::from("/run/user/1000/cortexfs/agent/scope/coder.sock")
        );
    }

    #[test]
    fn component_validation_rejects_path_escape() {
        assert!(cortexfs_paths::is_component("coder"));
        assert!(!cortexfs_paths::is_component("../coder"));
        assert!(!cortexfs_paths::is_component("coder/sock"));
        assert!(!cortexfs_paths::is_component(""));
        assert!(!cortexfs_paths::is_component("coder\0sock"));
    }
}
