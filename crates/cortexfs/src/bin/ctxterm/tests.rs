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
                agent: "coder".into(),
                session: "session-1".into(),
                unit: "cortexfs-agent-coder-session-1-terminal".into(),
            },
            program: OsString::from("/usr/bin/tsh"),
            args: Vec::new(),
        }
    }

    #[test]
    fn ctxterm_requires_broker_identity() {
        assert!(parse_args(Vec::new()).is_err());
        assert!(parse_args(["/usr/bin/tsh"].map(OsString::from).to_vec()).is_err());
        assert!(parse_args(["--broker", "coder"].map(OsString::from).to_vec()).is_err());
    }

    #[test]
    fn ctxterm_parses_broker_identity_and_command() {
        assert_eq!(
            parse_args(
                [
                    "--broker",
                    "coder",
                    "session-1",
                    "cortexfs-agent-coder-session-1-terminal",
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
    fn ctxterm_pty_command_uses_allowlisted_environment() -> Result<(), CtxtermError> {
        let command = pty_command_with_env(
            &config(),
            [
                (OsString::from("CTX_AGENT"), OsString::from("coder")),
                (OsString::from("HOME"), OsString::from("/home/agent")),
                (OsString::from("PATH"), OsString::from("/tmp/evil")),
                (OsString::from("LD_PRELOAD"), OsString::from("evil.so")),
            ],
        )?;
        let env = command
            .iter_full_env_as_str()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect::<Vec<_>>();
        assert!(env.contains(&("CTX_AGENT".into(), "coder".into())));
        assert!(env.contains(&("HOME".into(), "/home/agent".into())));
        assert!(env.contains(&("PATH".into(), "/usr/bin:/bin".into())));
        assert!(!env.iter().any(|entry| entry.0 == "LD_PRELOAD"));
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
