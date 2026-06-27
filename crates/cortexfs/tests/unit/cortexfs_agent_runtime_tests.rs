use super::{runtime_agent_executable, RuntimeConfig, DEFAULT_SOURCE};
use std::ffi::OsString;
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
