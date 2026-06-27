use super::{runtime_agent_executable, RuntimeConfig, DEFAULT_SOURCE};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::path::PathBuf;

#[test]
fn runtime_config_parses_agent_and_default_source() {
    let parsed = RuntimeConfig::parse(vec![OsString::from("--agent"), OsString::from("coder")]);
    assert_eq!(
        parsed,
        Ok(RuntimeConfig {
            source: Path::new(DEFAULT_SOURCE).to_path_buf(),
            agent: "coder".to_owned(),
        })
    );
}

#[test]
fn runtime_config_accepts_positional_agent() {
    let parsed = RuntimeConfig::parse(vec![
        OsString::from("--source"),
        OsString::from("/tmp/ctx"),
        OsString::from("reviewer"),
    ]);
    assert_eq!(
        parsed,
        Ok(RuntimeConfig {
            source: Path::new("/tmp/ctx").to_path_buf(),
            agent: "reviewer".to_owned(),
        })
    );
}

#[test]
fn runtime_credential_name_uses_object_name_components() {
    assert_eq!(
        super::safe_runtime_credential_name("../agent", "default"),
        Err("runtime credential path components must be object names".to_owned())
    );
    assert_eq!(
        super::safe_runtime_credential_name("agent", "../default"),
        Err("runtime credential path components must be object names".to_owned())
    );
    assert_eq!(
        super::safe_runtime_credential_name(".", "default"),
        Err("runtime credential path components must be object names".to_owned())
    );
    assert_eq!(
        super::safe_runtime_credential_name("agent", ".."),
        Err("runtime credential path components must be object names".to_owned())
    );
    assert_eq!(
        super::safe_runtime_credential_name("", "default"),
        Err("runtime credential path components must be object names".to_owned())
    );
    assert_eq!(
        super::safe_runtime_credential_name("coder", "default"),
        Ok("coder-provider-default".to_owned())
    );
}

#[test]
fn runtime_agent_executable_uses_ctx_abi_path() {
    assert_eq!(
        runtime_agent_executable(Path::new("/ctx"), "coder"),
        PathBuf::from("/ctx/agent/coder")
    );
}

#[test]
fn runtime_credential_dir_repair_sets_agent_owner_and_private_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runtime-credential-dir-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755))?;
    let fd = super::open_dir_no_follow(&root)?;
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();

    super::repair_runtime_credential_dir(&fd, uid, gid)?;

    let metadata = fs::metadata(&root)?;
    assert_eq!(metadata.uid(), uid);
    assert_eq!(metadata.gid(), gid);
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn session_permission_repair_skips_non_file_special_entries()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-session-special-repair-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let socket = root.join("runtime.sock");
    let _listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o777))?;
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();

    super::repair_path_permissions(&root, uid, gid)?;

    assert_eq!(
        fs::symlink_metadata(&socket)?.permissions().mode() & 0o777,
        0o777
    );
    let _ignored = fs::remove_file(&socket);
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn session_permission_repair_skips_dangling_symlink_root() -> Result<(), Box<dyn std::error::Error>>
{
    let root = std::env::temp_dir().join(format!(
        "cortexfs-session-dangling-symlink-repair-{}",
        std::process::id()
    ));
    let target = std::env::temp_dir().join(format!(
        "cortexfs-session-dangling-symlink-repair-target-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_file(&root);
    let _ignored = fs::remove_dir_all(&target);
    std::os::unix::fs::symlink(&target, &root)?;
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();

    super::repair_agent_session_permissions(&root, uid, gid)?;

    assert!(root.symlink_metadata()?.file_type().is_symlink());
    assert!(!target.exists());
    let _ignored = fs::remove_file(root);
    Ok(())
}

#[test]
fn session_permission_repair_rejects_symlink_intermediate_without_chmodding_target(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-session-intermediate-symlink-repair-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-session-intermediate-symlink-repair-outside-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    let _ignored = fs::remove_dir_all(&outside);
    fs::create_dir_all(&root)?;
    fs::create_dir_all(outside.join("session"))?;
    let target = outside.join("session").join("state");
    fs::write(&target, "outside\n")?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o666))?;
    std::os::unix::fs::symlink(&outside, root.join("link"))?;
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();

    assert!(super::repair_path_permissions(&root.join("link").join("session"), uid, gid).is_err());
    assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o666);

    let _ignored = fs::remove_dir_all(root);
    let _ignored = fs::remove_dir_all(outside);
    Ok(())
}

#[test]
fn session_permission_repair_recurses_without_chmodding_symlink_targets(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-session-recursive-repair-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-session-recursive-repair-outside-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    let _ignored = fs::remove_dir_all(&outside);
    fs::create_dir_all(root.join("default"))?;
    fs::create_dir_all(&outside)?;
    let child = root.join("default").join("state");
    let target = outside.join("state");
    fs::write(&child, "idle\n")?;
    fs::write(&target, "outside\n")?;
    fs::set_permissions(&child, fs::Permissions::from_mode(0o666))?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o666))?;
    std::os::unix::fs::symlink(&target, root.join("default").join("link"))?;
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();

    super::repair_path_permissions(&root, uid, gid)?;

    assert_eq!(fs::metadata(&child)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o666);

    let _ignored = fs::remove_dir_all(root);
    let _ignored = fs::remove_dir_all(outside);
    Ok(())
}

#[test]
fn runtime_provider_secret_file_is_removed_on_drop() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runtime-secret-drop-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let path = root.join("coder-provider-default");
    fs::write(&path, "secret\n")?;

    {
        let _secret = super::RuntimeProviderSecretFile {
            dir_fd: super::open_dir_no_follow(&root)?,
            file_name: "coder-provider-default".to_owned(),
            path: path.clone(),
            provider: "local".to_owned(),
            account: "default".to_owned(),
        };
        assert!(path.exists());
    }

    assert!(!path.exists());
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn runtime_provider_secret_drop_unlinks_original_directory_entry(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runtime-secret-drop-fd-{}",
        std::process::id()
    ));
    let moved = std::env::temp_dir().join(format!(
        "cortexfs-runtime-secret-drop-fd-moved-{}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "cortexfs-runtime-secret-drop-fd-outside-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    let _ignored = fs::remove_dir_all(&moved);
    let _ignored = fs::remove_dir_all(&outside);
    fs::create_dir_all(&root)?;
    fs::create_dir_all(&outside)?;
    let file_name = "coder-provider-default";
    let original = root.join(file_name);
    fs::write(&original, "secret\n")?;
    let dir_fd = super::open_dir_no_follow(&root)?;
    fs::rename(&root, &moved)?;
    fs::write(outside.join(file_name), "outside\n")?;
    std::os::unix::fs::symlink(&outside, &root)?;

    {
        let _secret = super::RuntimeProviderSecretFile {
            dir_fd,
            file_name: file_name.to_owned(),
            path: original,
            provider: "local".to_owned(),
            account: "default".to_owned(),
        };
    }

    assert!(!moved.join(file_name).exists());
    assert_eq!(
        fs::read_to_string(outside.join(file_name)).unwrap_or_default(),
        "outside\n"
    );
    let _ignored = fs::remove_file(&root);
    let _ignored = fs::remove_dir_all(moved);
    let _ignored = fs::remove_dir_all(outside);
    Ok(())
}
