use super::{DEFAULT_SOURCE, RuntimeConfig, runtime_model};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::path::PathBuf;

#[test]
pub(crate) fn runtime_config_parses_agent_and_default_source() {
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
pub(crate) fn runtime_config_accepts_positional_agent() {
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
pub(crate) fn runtime_agent_executable_uses_ctx_abi_path() {
    assert_eq!(
        Path::new("/ctx").join("agent").join("coder"),
        PathBuf::from("/ctx/agent/coder")
    );
}

#[test]
pub(crate) fn runtime_model_keeps_requested_model_without_primary_secret()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "cortexfs-runtime-model-no-implicit-fallback-{}",
        std::process::id()
    ));
    let _ignored = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("model/remote"))?;
    fs::create_dir_all(root.join("model/mirror"))?;
    std::os::unix::fs::symlink("/ctx/model/remote/alpha", root.join("model/main"))?;
    fs::write(root.join("model/mirror/alpha"), "#!/bin/sh\n")?;

    let model = runtime_model(&root, "main");

    assert_eq!(model, "main");
    let _ignored = fs::remove_dir_all(root);
    Ok(())
}

#[test]
pub(crate) fn session_permission_repair_skips_non_file_special_entries()
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
pub(crate) fn session_permission_repair_skips_dangling_symlink_root()
-> Result<(), Box<dyn std::error::Error>> {
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
pub(crate) fn session_permission_repair_rejects_symlink_intermediate_without_chmodding_target()
-> Result<(), Box<dyn std::error::Error>> {
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
pub(crate) fn session_permission_repair_recurses_without_chmodding_symlink_targets()
-> Result<(), Box<dyn std::error::Error>> {
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
