use super::{
    BWRAP_PROGRAM, DEFAULT_SOURCE, RuntimeConfig, RuntimeMode, runtime_agent_environment,
    runtime_model,
};
use cortexfs::{MountTable, RunEnvironment};
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
            mode: RuntimeMode::Serve,
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
            mode: RuntimeMode::Serve,
        })
    );
}

#[test]
pub(crate) fn runtime_config_parses_internal_socket_alias_modes() {
    for (flag, mode) in [
        ("--prepare-socket-alias", RuntimeMode::PrepareSocketAlias),
        ("--cleanup-socket-alias", RuntimeMode::CleanupSocketAlias),
    ] {
        let parsed = RuntimeConfig::parse(vec![
            OsString::from(flag),
            OsString::from("--source"),
            OsString::from("/tmp/ctx"),
            OsString::from("--agent"),
            OsString::from("coder"),
        ]);
        assert_eq!(
            parsed,
            Ok(RuntimeConfig {
                source: Path::new("/tmp/ctx").to_path_buf(),
                agent: "coder".to_owned(),
                mode,
            })
        );
    }
}

#[test]
pub(crate) fn runtime_config_rejects_conflicting_or_positional_internal_modes() {
    assert!(
        RuntimeConfig::parse(vec![
            OsString::from("--prepare-socket-alias"),
            OsString::from("--cleanup-socket-alias"),
            OsString::from("--agent"),
            OsString::from("coder"),
        ])
        .is_err()
    );
    assert!(
        RuntimeConfig::parse(vec![
            OsString::from("--prepare-socket-alias"),
            OsString::from("coder"),
        ])
        .is_err()
    );
}

#[test]
pub(crate) fn packaged_socket_unit_uses_receipted_alias_lifecycle_and_safe_ordering()
-> Result<(), Box<dyn std::error::Error>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let unit = fs::read_to_string(manifest.join("../../packaging/systemd/cortexfs-agent@.socket"))?;
    for expected in [
        "DefaultDependencies=no",
        "Requires=cortexfs.service",
        "After=cortexfs.service",
        "Conflicts=shutdown.target",
        "Before=shutdown.target",
        "--prepare-socket-alias",
        "--cleanup-socket-alias",
    ] {
        assert!(unit.lines().any(|line| line.contains(expected)));
    }
    assert!(!unit.contains("BindsTo=cortexfs.service"));
    assert!(!unit.contains("PartOf=cortexfs.service"));
    assert!(!unit.contains("/usr/bin/rm -f"));
    assert!(!unit.contains("/usr/bin/ln -s"));

    let service =
        fs::read_to_string(manifest.join("../../packaging/systemd/cortexfs-agent@.service"))?;
    for expected in [
        "BindsTo=cortexfs.service",
        "PartOf=cortexfs.service",
        "After=cortexfs.service",
    ] {
        assert!(service.lines().any(|line| line == expected));
    }
    let unit_section = service
        .strip_prefix("[Unit]\n")
        .and_then(|contents| contents.split_once("\n[Service]\n"))
        .ok_or("packaged agent service must contain [Unit] before [Service]")?;
    assert_eq!(
        unit_section
            .0
            .lines()
            .filter(|line| *line == "StartLimitIntervalSec=0")
            .count(),
        1
    );
    assert!(!unit_section.1.contains("StartLimitIntervalSec"));
    Ok(())
}

#[test]
pub(crate) fn runtime_agent_environment_uses_bwrap_sandbox()
-> Result<(), Box<dyn std::error::Error>> {
    let mount_table = MountTable::parse("/ctx\t/ctx\tro\trbind,nosuid,nodev\n")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let environment = runtime_agent_environment(&mount_table, Path::new("/run/cortexfs/control"));
    assert_eq!(environment.kind(), "sandbox");
    let RunEnvironment::Sandbox {
        program,
        mount_table: selected_mount_table,
        ..
    } = environment
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

#[test]
fn codex_runtime_environment_contains_no_refresh_token() {
    let env = super::secret_runtime_env(
        "access".to_owned(),
        "codex".to_owned(),
        "default".to_owned(),
        "account".to_owned(),
    );
    assert_eq!(env.last().map(|value| value.1.as_str()), Some("account"));
    assert!(!format!("{env:?}").contains("refresh"));
}
