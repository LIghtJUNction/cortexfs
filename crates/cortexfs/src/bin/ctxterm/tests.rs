use crate::{
    BrokerConfig, CtxtermCommand, RunConfig, env_u16_from_value, parse_args, pty_command_with_env,
};
use std::ffi::OsString;

fn config() -> RunConfig {
    RunConfig {
        broker: BrokerConfig {
            agent: "executor".into(),
            session: "session-1".into(),
            unit: "cortexfs-agent-executor-session-1-terminal".into(),
        },
        program: OsString::from("/usr/bin/tsh"),
        args: Vec::new(),
    }
}

#[test]
fn ctxterm_requires_broker_identity() {
    assert!(parse_args(Vec::new()).is_err());
    assert!(parse_args(["/usr/bin/tsh"].map(OsString::from).to_vec()).is_err());
    assert!(parse_args(["--broker", "executor"].map(OsString::from).to_vec()).is_err());
}

#[test]
fn ctxterm_parses_broker_identity_and_command() {
    assert_eq!(
        parse_args(
            [
                "--broker",
                "executor",
                "session-1",
                "cortexfs-agent-executor-session-1-terminal",
                "--",
                "/usr/bin/tsh",
                "--login",
            ]
            .map(OsString::from)
            .to_vec(),
        ),
        Ok(CtxtermCommand::Run {
            broker: config().broker,
            program: OsString::from("/usr/bin/tsh"),
            args: vec![OsString::from("--login")],
        })
    );
}

#[test]
fn ctxterm_pty_child_preserves_process_contract() -> Result<(), Box<dyn std::error::Error>> {
    let expected = [
        ("CTX_AGENT", "executor"),
        ("HOME", "/home/agent"),
        ("PATH", "/opt/agent/bin"),
        ("XDG_CONFIG_HOME", "/home/agent/.config"),
        ("AGENT_CLI_TOKEN", "explicit"),
    ];
    let mut config = config();
    config.program = "/bin/sh".into();
    config.args = [
        "-c",
        r#"[ "$#" = 4 ] || exit 91; [ "$1" = 'two words' ] || exit 92
[ -z "$2" ] || exit 93; [ "$3" = 'λ;*' ] || exit 94
[ "$PWD" = "$4" ] || exit 95; [ "$AGENT_CLI_TOKEN" = explicit ] || exit 96
exit 23"#,
        "hosted-cli",
        "two words",
        "",
        "λ;*",
    ]
    .map(OsString::from)
    .to_vec();
    config.args.push(std::env::current_dir()?.into_os_string());
    let command = pty_command_with_env(
        &config,
        expected.map(|(key, value)| (OsString::from(key), OsString::from(value))),
    )
    .map_err(|error| std::io::Error::other(error.message))?;
    for pair in expected {
        assert!(command.iter_full_env_as_str().any(|entry| entry == pair));
    }
    let pair = portable_pty::native_pty_system().openpty(crate::pty_size())?;
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let status = child.wait()?;
    assert_eq!(status.exit_code(), 23);
    assert_eq!(crate::exit_code(&status), std::process::ExitCode::from(23));
    Ok(())
}

#[test]
fn ctxterm_env_u16_rejects_zero_and_invalid_values() {
    assert_eq!(env_u16_from_value(Some("24")), Some(24));
    assert_eq!(env_u16_from_value(Some("0")), None);
    assert_eq!(env_u16_from_value(Some("bad")), None);
    assert_eq!(env_u16_from_value(None), None);
}
