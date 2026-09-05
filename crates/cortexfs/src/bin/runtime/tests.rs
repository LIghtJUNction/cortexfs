use super::{BWRAP_PROGRAM, DEFAULT_SOURCE, RuntimeConfig, RuntimeMode, runtime_agent_environment};
use cortexfs::{MountTable, RunEnvironment};
use std::{ffi::OsString, fs, path::Path};

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
#[test]
fn runtime_config_preserves_serve_and_alias_commands() {
    let expected = [
        (
            &["--agent", "executor"][..],
            RuntimeConfig {
                source: Path::new(DEFAULT_SOURCE).into(),
                agent: "executor".into(),
                mode: RuntimeMode::Serve,
            },
        ),
        (
            &["--source", "/tmp/ctx", "reviewer"][..],
            RuntimeConfig {
                source: Path::new("/tmp/ctx").into(),
                agent: "reviewer".into(),
                mode: RuntimeMode::Serve,
            },
        ),
        (
            &["--prepare-socket-alias", "--agent", "executor"][..],
            RuntimeConfig {
                source: Path::new(DEFAULT_SOURCE).into(),
                agent: "executor".into(),
                mode: RuntimeMode::PrepareSocketAlias,
            },
        ),
        (
            &["--cleanup-socket-alias", "--agent", "executor"][..],
            RuntimeConfig {
                source: Path::new(DEFAULT_SOURCE).into(),
                agent: "executor".into(),
                mode: RuntimeMode::CleanupSocketAlias,
            },
        ),
    ];
    for (input, config) in expected {
        assert_eq!(RuntimeConfig::parse(args(input)), Ok(config));
    }
    for input in [
        &[
            "--prepare-socket-alias",
            "--cleanup-socket-alias",
            "--agent",
            "executor",
        ][..],
        &["--prepare-socket-alias", "executor"][..],
    ] {
        assert!(RuntimeConfig::parse(args(input)).is_err());
    }
}
fn assert_unit_lines(unit: &str, expected: &[&str]) {
    for line in expected {
        assert!(
            unit.lines().any(|candidate| candidate.contains(line)),
            "missing {line}"
        );
    }
}
#[test]
fn packaged_socket_unit_preserves_safe_service_lifecycle() -> Result<(), Box<dyn std::error::Error>>
{
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/systemd");
    let socket = fs::read_to_string(manifest.join("cortexfs-agent@.socket"))?;
    assert_unit_lines(
        &socket,
        &[
            "DefaultDependencies=no",
            "Requires=cortexfs.service",
            "After=cortexfs.service",
            "Conflicts=shutdown.target",
            "Before=shutdown.target",
            "--prepare-socket-alias",
            "--cleanup-socket-alias",
        ],
    );
    for forbidden in [
        "BindsTo=cortexfs.service",
        "PartOf=cortexfs.service",
        "/usr/bin/rm -f",
        "/usr/bin/ln -s",
    ] {
        assert!(!socket.contains(forbidden));
    }
    let service = fs::read_to_string(manifest.join("cortexfs-agent@.service"))?;
    assert_unit_lines(
        &service,
        &[
            "BindsTo=cortexfs.service",
            "PartOf=cortexfs.service",
            "After=cortexfs.service",
            "MemoryMax=512M",
            "CPUQuota=100%",
            "TasksMax=128",
            "OOMPolicy=stop",
            "NoNewPrivileges=yes",
            "PrivateTmp=yes",
            "RestrictNamespaces=mnt ipc net pid uts user cgroup",
            "ProtectHostname=yes",
        ],
    );
    let (unit, body) = service
        .strip_prefix("[Unit]\n")
        .and_then(|value| value.split_once("\n[Service]\n"))
        .ok_or("packaged agent service must contain [Unit] before [Service]")?;
    assert_eq!(
        unit.lines()
            .filter(|line| *line == "StartLimitIntervalSec=0")
            .count(),
        1
    );
    assert!(!body.contains("StartLimitIntervalSec"));
    for directive in [
        "RestrictNamespaces=mnt ipc net pid uts user cgroup",
        "ProtectHostname=yes",
    ] {
        let key = directive.split('=').next().ok_or("missing directive key")?;
        assert_eq!(
            body.lines()
                .filter(|line| line.starts_with(&format!("{key}=")))
                .collect::<Vec<_>>(),
            [directive]
        );
    }
    Ok(())
}
#[test]
fn runtime_environment_preserves_sandbox_model_and_secret_behavior()
-> Result<(), Box<dyn std::error::Error>> {
    let mounts = MountTable::parse("/ctx\t/ctx\tro\trbind,nosuid,nodev\n")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let environment = runtime_agent_environment(&mounts, Path::new("/run/cortexfs/control"));
    let RunEnvironment::Sandbox {
        program,
        mount_table,
        ..
    } = environment
    else {
        return Err("socket-activated agents must use the bwrap sandbox".into());
    };
    assert_eq!(
        (environment.kind(), program, mount_table.entries()),
        ("sandbox", Path::new(BWRAP_PROGRAM), mounts.entries())
    );
    let env = super::secret_runtime_env(
        "access".into(),
        "codex".into(),
        "default".into(),
        "account".into(),
    );
    assert_eq!(env.last().map(|value| value.1.as_str()), Some("account"));
    assert!(!format!("{env:?}").contains("refresh"));
    Ok(())
}
