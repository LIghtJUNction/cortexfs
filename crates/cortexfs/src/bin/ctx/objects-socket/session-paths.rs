use crate::*;

pub(crate) fn request_id() -> Result<String, CliError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CliError::unavailable(format!("system clock before epoch: {error}")))?
        .as_millis();
    Ok(format!("ctx-{millis}"))
}

pub(crate) fn agent_session_dir(
    root: &Path,
    agent: &str,
    session: Option<&str>,
) -> Result<PathBuf, CliError> {
    let session = agent_session_name(root, agent, session)?;
    Ok(ctx_home(root)?
        .join("agent")
        .join(agent)
        .join("session")
        .join(session))
}

pub(crate) fn agent_session_name(
    root: &Path,
    agent: &str,
    session: Option<&str>,
) -> Result<String, CliError> {
    require_cli_name("agent name", agent)?;
    if let Some(session) = session {
        require_cli_name("session name", session)?;
    }

    let session_root = ctx_home(root)?.join("agent").join(agent).join("session");
    Ok(match session {
        Some(name) => name.to_owned(),
        None => current_session_name(&session_root)?,
    })
}

pub(crate) fn agent_socket_path(root: &Path, agent: &str) -> Result<PathBuf, CliError> {
    require_cli_name("agent name", agent)?;
    if agent_user_control_dir(root, agent)
        .as_deref()
        .is_some_and(is_plain_dir)
    {
        return Ok(ctx_home(root)?.join("agent").join(format!("{agent}.sock")));
    }
    Ok(root.join("agent").join(format!("{agent}.sock")))
}

pub(crate) fn require_cli_name(label: &str, value: &str) -> Result<(), CliError> {
    if is_object_name(value) {
        Ok(())
    } else {
        Err(CliError::usage(format!("invalid {label}: {value}")))
    }
}

pub(crate) fn object_socket_path(root: &Path, path: &str) -> Result<PathBuf, CliError> {
    let abi_path = classify_input_path(root, path)?;
    if !matches!(
        classify_abi_path(&abi_path),
        "ctx.model.exec" | "ctx.agent.exec"
    ) {
        return Err(CliError::usage(format!(
            "socket command requires model/NAME or agent/NAME: {path}"
        )));
    }

    let Some((class, name)) = abi_path.split_once('/') else {
        return Err(CliError::usage(format!("invalid object path: {path}")));
    };
    Ok(root.join(class).join(format!("{name}.sock")))
}

pub(crate) fn current_session_name(session_root: &Path) -> Result<String, CliError> {
    let current_path = session_root.join("index").join("current");
    let current = read_small_plain_text_file(&current_path, 64 * 1024, "current session file");

    match current {
        Ok(value) => {
            let session = value.trim();
            if is_object_name(session) {
                if session != "default" && !plain_session_dir_exists(&session_root.join(session)) {
                    return Ok("default".to_owned());
                }
                Ok(session.to_owned())
            } else {
                Err(CliError::unavailable(format!(
                    "invalid current session in {}",
                    current_path.display()
                )))
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
            ) =>
        {
            Ok("default".to_owned())
        }
        Err(error) => Err(CliError::unavailable(format!(
            "cannot read {}: {error}",
            current_path.display()
        ))),
    }
}

pub(crate) fn plain_session_dir_exists(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

pub(crate) fn ctx_home(root: &Path) -> Result<PathBuf, CliError> {
    if let Some(home) = env::var_os("CTX_HOME") {
        return Ok(PathBuf::from(home));
    }

    Ok(root
        .join("home")
        .join(current_uid_text().map_err(CliError::unavailable)?))
}

#[cfg(test)]
mod objects_socket_id_program_tests {
    use super::{get_id_program, id_command, parse_current_uid_text};

    #[test]
    fn get_id_program_returns_absolute_path() {
        assert_eq!(get_id_program(), "/usr/bin/id");
    }

    #[test]
    fn id_command_uses_clean_runtime_environment() {
        let command = id_command();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut envs = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<Vec<_>>();
        envs.sort();

        assert_eq!(command.get_program(), "/usr/bin/id");
        assert_eq!(args, vec!["-u".to_owned()]);
        assert_eq!(
            envs,
            vec![("PATH".to_owned(), Some("/usr/bin:/bin".to_owned()))]
        );
    }

    #[test]
    fn parse_current_uid_accepts_digits_only() {
        assert_eq!(parse_current_uid_text("1000\n"), Ok("1000".to_owned()));
        assert!(parse_current_uid_text("1000\n1001\n").is_err());
        assert!(parse_current_uid_text("user\n").is_err());
        assert!(parse_current_uid_text("\n").is_err());
    }
}
