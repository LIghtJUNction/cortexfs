use super::ProviderEgressError;

#[derive(Eq, PartialEq)]
pub(super) struct ProviderEgressCredential {
    pub(super) token: String,
    pub(super) codex_account_id: Option<String>,
    pub(super) run: String,
}

pub(super) fn provider_egress_credential(
    environment: &[(String, String)],
    provider: &str,
    run: &str,
) -> Result<Option<ProviderEgressCredential>, ProviderEgressError> {
    if runtime_env_value(environment, "CTX_PROVIDER_SECRET_PROVIDER") != Some(provider) {
        return Ok(None);
    }
    let Some(token) = runtime_env_value(environment, "CTX_PROVIDER_SECRET_VALUE")
        .map(|value| value.trim_end_matches(['\r', '\n']))
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let codex_account_id = runtime_env_value(environment, "CTX_PROVIDER_SECRET_ACCOUNT_ID")
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    if token.chars().any(char::is_control)
        || codex_account_id
            .as_deref()
            .is_some_and(|value| value.chars().any(char::is_control))
        || (provider == "codex" && codex_account_id.is_none())
    {
        return Err(ProviderEgressError::CannotCreate);
    }
    Ok(Some(ProviderEgressCredential {
        token: token.to_owned(),
        codex_account_id,
        run: run.to_owned(),
    }))
}

fn runtime_env_value<'a>(environment: &'a [(String, String)], name: &str) -> Option<&'a str> {
    environment
        .iter()
        .rev()
        .find(|entry| entry.0 == name)
        .map(|entry| entry.1.as_str())
}
