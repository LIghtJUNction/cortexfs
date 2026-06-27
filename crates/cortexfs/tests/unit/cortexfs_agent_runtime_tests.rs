use super::{runtime_agent_executable, RuntimeConfig, DEFAULT_SOURCE};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
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
fn runtime_credential_name_rejects_path_separators() {
    assert_eq!(
        super::safe_runtime_credential_name("../agent", "default"),
        Err("runtime credential path components must not contain '/'".to_owned())
    );
    assert_eq!(
        super::safe_runtime_credential_name("agent", "../default"),
        Err("runtime credential path components must not contain '/'".to_owned())
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
