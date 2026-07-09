use crate::*;

#[cfg(test)]
mod provider_secret_file_tests {
    use super::super::secret_files::set_private_dir_permissions;
    use super::super::{
        create_private_provider_secret_dir, is_secret_account_name, open_provider_secret_file,
        provider_host_from_base_url, provider_secret_file_exists, read_provider_secret_file,
        selected_model_provider,
    };
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn provider_secret_account_names_use_object_name_rules() {
        assert!(is_secret_account_name("default"));
        assert!(is_secret_account_name("office.prod"));
        assert!(!is_secret_account_name(""));
        assert!(!is_secret_account_name("."));
        assert!(!is_secret_account_name(".."));
        assert!(!is_secret_account_name("../default"));
        assert!(!is_secret_account_name("bad/name"));
        assert!(!is_secret_account_name("-bad"));
    }

    #[test]
    fn provider_base_url_host_requires_http_scheme_and_clean_text() {
        assert_eq!(
            provider_host_from_base_url("https://api.openai.com/v1"),
            Some("api.openai.com".to_owned())
        );
        assert_eq!(
            provider_host_from_base_url("http://127.0.0.1:8317/v1"),
            Some("127.0.0.1".to_owned())
        );
        assert_eq!(provider_host_from_base_url("api.openai.com/v1"), None);
        assert_eq!(provider_host_from_base_url("https:///v1"), None);
        assert_eq!(
            provider_host_from_base_url("https://api.openai.com\noutput=/tmp/leak"),
            None
        );
    }

    #[test]
    fn provider_secret_file_helpers_refuse_symlink_targets()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let target = root.join("target");
        let link = root.join("link");
        fs::write(&target, "secret\n")?;
        symlink(&target, &link)?;

        assert!(read_provider_secret_file(&link).is_err());
        assert!(open_provider_secret_file(&link).is_err());
        assert_eq!(
            provider_secret_file_exists(&link),
            Err(super::ProviderSystemSecretError::CannotRead)
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_file_helpers_reject_symlink_intermediate_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-symlink-parent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outside = root.join("outside");
        fs::create_dir_all(outside.join("local"))?;
        fs::write(outside.join("local/default"), "secret\n")?;
        symlink(&outside, root.join("provider"))?;

        let path = root.join("provider").join("local").join("default");
        assert!(read_provider_secret_file(&path).is_err());
        assert!(open_provider_secret_file(&path).is_err());
        assert_eq!(
            provider_secret_file_exists(&path),
            Err(super::ProviderSystemSecretError::CannotRead)
        );
        assert_eq!(
            fs::read_to_string(outside.join("local/default"))?,
            "secret\n"
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn selected_model_provider_rejects_symlink_model_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-selected-model-provider-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outside = root.join("outside");
        fs::create_dir_all(&outside)?;
        symlink("/ctx/model/evil/model", outside.join("main"))?;
        symlink(&outside, root.join("model"))?;

        assert_eq!(selected_model_provider(&root, "main"), None);

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_file_helpers_read_plain_files() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-plain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let path = root.join("default");
        fs::write(&path, "secret\n")?;

        assert_eq!(read_provider_secret_file(&path)?, "secret\n");
        assert!(open_provider_secret_file(&path).is_ok());
        assert_eq!(provider_secret_file_exists(&path), Ok(true));
        assert_eq!(
            provider_secret_file_exists(&root.join("missing")),
            Ok(false)
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_file_helpers_reject_non_regular_and_oversized_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let dir = root.join("dir");
        fs::create_dir_all(&dir)?;
        let oversized = root.join("oversized");
        fs::write(&oversized, "x".repeat((64 * 1024) + 1))?;

        assert!(open_provider_secret_file(&dir).is_err());
        assert!(read_provider_secret_file(&dir).is_err());
        assert!(open_provider_secret_file(&oversized).is_err());
        assert!(read_provider_secret_file(&oversized).is_err());

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_private_dir_permissions_repair_plain_directories()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-dir-plain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let target = root.join("target");
        fs::create_dir_all(&target)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;

        assert_eq!(set_private_dir_permissions(&target), Ok(()));
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o700);

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_private_dir_permissions_refuse_symlink_directories()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-dir-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let target = root.join("target");
        let link = root.join("link");
        fs::create_dir_all(&target)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
        symlink(&target, &link)?;

        assert_eq!(
            set_private_dir_permissions(&link),
            Err(super::ProviderSystemSecretError::CannotWrite)
        );
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o755);

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_private_dir_permissions_reject_symlink_intermediate_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-dir-symlink-parent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outside = root.join("outside");
        fs::create_dir_all(outside.join("local"))?;
        fs::set_permissions(outside.join("local"), fs::Permissions::from_mode(0o755))?;
        symlink(&outside, root.join("provider"))?;

        assert_eq!(
            set_private_dir_permissions(&root.join("provider").join("local")),
            Err(super::ProviderSystemSecretError::CannotWrite)
        );
        assert_eq!(
            fs::metadata(outside.join("local"))?.permissions().mode() & 0o777,
            0o755
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_dir_creation_sets_private_modes() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-create-private-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let path = root.join("secrets").join("provider").join("local");

        assert_eq!(create_private_provider_secret_dir(&path), Ok(()));
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(root.join("secrets"))?.permissions().mode() & 0o777,
            0o700
        );

        let _ignored = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn provider_secret_dir_creation_refuses_symlink_parent()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-create-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outside = std::env::temp_dir().join(format!(
            "cortexfs-provider-secret-create-symlink-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(root.join("secrets"))?;
        fs::create_dir_all(&outside)?;
        symlink(&outside, root.join("secrets").join("provider"))?;
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755))?;

        assert_eq!(
            create_private_provider_secret_dir(
                &root.join("secrets").join("provider").join("local")
            ),
            Err(super::ProviderSystemSecretError::CannotWrite)
        );
        assert!(!outside.join("local").exists());
        assert_eq!(fs::metadata(&outside)?.permissions().mode() & 0o777, 0o755);

        let _ignored = fs::remove_dir_all(root);
        let _ignored = fs::remove_dir_all(outside);
        Ok(())
    }
}
