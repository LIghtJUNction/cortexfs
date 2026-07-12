use super::{BWRAP_PROGRAM, DEFAULT_SOURCE, RuntimeConfig, runtime_agent_execution, runtime_model};
use cortexfs::{AgentExecutableSocketExecution, MountTable};
use std::ffi::OsString;
use std::fs;
use std::path::Path;

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
pub(crate) fn runtime_agent_execution_uses_bwrap_sandbox() -> Result<(), Box<dyn std::error::Error>>
{
    let mount_table = MountTable::parse("/ctx\t/ctx\tro\trbind,nosuid,nodev\n")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let AgentExecutableSocketExecution::Bwrap {
        program,
        mount_table: selected_mount_table,
        ..
    } = runtime_agent_execution(&mount_table, Path::new("/run/cortexfs/control"))
    else {
        return Err("socket-activated agents must use the bwrap sandbox".into());
    };
    assert_eq!(program, Path::new(BWRAP_PROGRAM));
    assert_eq!(selected_mount_table.entries(), mount_table.entries());
    Ok(())
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
