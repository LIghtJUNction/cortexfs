use crate::*;

#[cfg(test)]
mod tests {
    use super::{
        BrokerConfig, CtxtermCommand, CtxtermError, RunConfig, env_u16_from_value, parse_args,
        pty_command_with_env,
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
    fn ctxterm_pty_command_forwards_boundary_environment() -> Result<(), CtxtermError> {
        let command = pty_command_with_env(
            &config(),
            [
                (OsString::from("CTX_AGENT"), OsString::from("executor")),
                (OsString::from("HOME"), OsString::from("/home/agent")),
                (OsString::from("PATH"), OsString::from("/opt/agent/bin")),
                (
                    OsString::from("XDG_CONFIG_HOME"),
                    OsString::from("/home/agent/.config"),
                ),
                (
                    OsString::from("AGENT_CLI_TOKEN"),
                    OsString::from("explicit"),
                ),
            ],
        )?;
        let env = command
            .iter_full_env_as_str()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect::<Vec<_>>();
        for expected in [
            ("CTX_AGENT", "executor"),
            ("HOME", "/home/agent"),
            ("PATH", "/opt/agent/bin"),
            ("XDG_CONFIG_HOME", "/home/agent/.config"),
            ("AGENT_CLI_TOKEN", "explicit"),
        ] {
            assert!(env.contains(&(expected.0.into(), expected.1.into())));
        }
        Ok(())
    }

    #[test]
    fn ctxterm_env_u16_rejects_zero_and_invalid_values() {
        assert_eq!(env_u16_from_value(Some("24")), Some(24));
        assert_eq!(env_u16_from_value(Some("0")), None);
        assert_eq!(env_u16_from_value(Some("bad")), None);
        assert_eq!(env_u16_from_value(None), None);
    }
}
