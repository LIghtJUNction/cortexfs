use crate::*;

/// Resolves an API key with the stable priority:
/// provider env candidates, system secret store, then unconfigured.
pub fn resolve_api_key(
    env_name: &str,
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    resolve_api_key_from_env_names(&[env_name.to_owned()], service, account)
}

/// Resolves an API key from candidate environment variables with the stable
/// priority: environment, system secret store, then unconfigured.
pub fn resolve_api_key_from_env_names(
    env_names: &[String],
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    resolve_api_key_from_env_names_with(
        env_names,
        service,
        account,
        |name| env::var(name),
        system_keychain_secret,
    )
}

/// Testable core for API key resolution from multiple environment candidates.
pub fn resolve_api_key_from_env_names_with<E, K>(
    env_names: &[String],
    service: &str,
    account: &str,
    env_lookup: E,
    keychain_lookup: K,
) -> Result<Option<String>, ApiKeyResolutionError>
where
    E: Fn(&str) -> Result<String, env::VarError>,
    K: FnOnce(&str, &str) -> Result<Option<String>, ApiKeyResolutionError>,
{
    if env_names.iter().any(|name| !is_valid_env_key(name))
        || !is_valid_secret_lookup_part(service)
        || !is_valid_secret_lookup_part(account)
    {
        return Err(ApiKeyResolutionError::InvalidName);
    }
    for env_name in env_names {
        match env_lookup(env_name) {
            Ok(value) if !value.trim().is_empty() => return Ok(Some(value)),
            Ok(_value) => {}
            Err(env::VarError::NotPresent) => {}
            Err(env::VarError::NotUnicode(_value)) => {
                return Err(ApiKeyResolutionError::InvalidName);
            }
        }
    }
    keychain_lookup(service, account)
}

/// Testable core for API key resolution.
pub fn resolve_api_key_with<E, K>(
    env_name: &str,
    service: &str,
    account: &str,
    env_lookup: E,
    keychain_lookup: K,
) -> Result<Option<String>, ApiKeyResolutionError>
where
    E: FnOnce(&str) -> Result<String, env::VarError>,
    K: FnOnce(&str, &str) -> Result<Option<String>, ApiKeyResolutionError>,
{
    if !is_valid_env_key(env_name)
        || !is_valid_secret_lookup_part(service)
        || !is_valid_secret_lookup_part(account)
    {
        return Err(ApiKeyResolutionError::InvalidName);
    }
    match env_lookup(env_name) {
        Ok(value) if !value.trim().is_empty() => return Ok(Some(value)),
        Ok(_value) => {}
        Err(env::VarError::NotPresent) => {}
        Err(env::VarError::NotUnicode(_value)) => {
            return Err(ApiKeyResolutionError::InvalidName);
        }
    }
    keychain_lookup(service, account)
}

pub(crate) fn system_keychain_secret(
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    let entry = match keyring::Entry::new(service, account) {
        Ok(entry) => entry,
        Err(keyring::Error::NoDefaultStore) => return secret_tool_lookup(service, account),
        Err(_error) => return secret_tool_lookup(service, account),
    };
    let secret = match entry.get_password() {
        Ok(secret) => secret,
        Err(keyring::Error::NoEntry) => return secret_tool_lookup(service, account),
        Err(_error) => return secret_tool_lookup(service, account),
    };
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret))
    }
}

pub(crate) const SECRET_TOOL_PROGRAM: &str = "/usr/bin/secret-tool";
const MAX_SECRET_TOOL_OUTPUT_BYTES: usize = 8 * 1024;
const SECRET_TOOL_TIMEOUT_SECONDS: u64 = 5;

pub(crate) fn secret_tool_lookup(
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    let mut command = Command::new(SECRET_TOOL_PROGRAM);
    command
        .env_clear()
        .args(["lookup", "service", service, "account", account])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.env(
        "DBUS_SESSION_BUS_ADDRESS",
        secret_tool_dbus_address(|name| env::var_os(name), nix::unistd::geteuid().as_raw()),
    );
    let output = match run_secret_tool_command_with_timeout(
        command,
        Duration::from_secs(SECRET_TOOL_TIMEOUT_SECONDS),
    ) {
        Ok(output) => output,
        Err(_error) => return Ok(None),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let secret =
        String::from_utf8(output.stdout).map_err(|_error| ApiKeyResolutionError::InvalidName)?;
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

pub(crate) fn run_secret_tool_command_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run secret-tool: {error}"))?;
    support::process::wait_capped_child_output(
        &mut child,
        support::process::CappedOutputWait {
            max_output_bytes: MAX_SECRET_TOOL_OUTPUT_BYTES,
            timeout,
            capture_stderr: false,
            drain_timeout: None,
            terminate_group_after_exit: false,
        },
        || false,
    )
    .map_err(|error| match error {
        support::process::CappedOutputError::ExceededLimit => {
            "secret-tool output exceeds limit".to_owned()
        }
        support::process::CappedOutputError::TimedOut => {
            format!("secret-tool timed out after {}s", timeout.as_secs())
        }
        support::process::CappedOutputError::Wait(error) => error.to_string(),
        support::process::CappedOutputError::Cancelled
        | support::process::CappedOutputError::DrainTimedOut => "secret-tool failed".to_owned(),
    })
}

pub(crate) fn secret_tool_dbus_address(
    get_env: impl FnOnce(&str) -> Option<std::ffi::OsString>,
    uid: u32,
) -> std::ffi::OsString {
    get_env("DBUS_SESSION_BUS_ADDRESS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("unix:path=/run/user/{uid}/bus").into())
}

pub(crate) fn is_valid_secret_lookup_part(value: &str) -> bool {
    !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_control())
}
