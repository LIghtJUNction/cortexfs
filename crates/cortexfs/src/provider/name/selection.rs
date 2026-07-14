use super::names::PROVIDER_SYSTEM_SECRET_ROOT;
use crate::*;
use std::net::IpAddr;

pub(crate) fn canonical_provider_name_from_host(host: &str) -> &str {
    match host {
        "api.openai.com" => "openai",
        "api.anthropic.com" => "anthropic",
        "generativelanguage.googleapis.com" => "google",
        _ => host,
    }
}

pub(crate) fn provider_host_requires_name(host: &str) -> bool {
    host == "localhost" || host.parse::<IpAddr>().is_ok()
}

/// Opens the default provider system secret for a selected model alias/name.
pub fn open_provider_system_secret_for_model(
    ctx_root: &Path,
    model: &str,
) -> Result<Option<ProviderSystemSecretHandle>, ProviderSystemSecretError> {
    let Some(provider) = selected_model_provider(ctx_root, model) else {
        return Ok(None);
    };
    open_provider_system_secret(&provider, "default")
}

/// Reads the default provider system secret for a selected model alias/name.
pub fn read_provider_system_secret_for_model(
    ctx_root: &Path,
    model: &str,
) -> Result<Option<ProviderSystemSecret>, ProviderSystemSecretError> {
    let Some(provider) = selected_model_provider(ctx_root, model) else {
        return Ok(None);
    };
    let account = "default";
    let Some(secret) = read_provider_system_secret(&provider, account)? else {
        return Ok(None);
    };
    Ok(Some(ProviderSystemSecret {
        provider,
        account: account.to_owned(),
        secret,
    }))
}

pub(crate) fn selected_model_provider(ctx_root: &Path, model: &str) -> Option<String> {
    let model = model.trim();
    if model.contains('/') {
        return model.split_once('/').and_then(|(provider, model)| {
            (!provider.is_empty() && !model.is_empty()).then_some(provider.to_owned())
        });
    }
    if !is_model_alias(model) {
        return None;
    }
    let target = read_model_alias_target(ctx_root, model).ok()?;
    let target = target
        .strip_prefix("/ctx/model/")
        .or_else(|| target.strip_prefix("model/"))
        .unwrap_or(&target);
    target.split_once('/').and_then(|(provider, model)| {
        (!provider.is_empty() && !model.is_empty()).then_some(provider.to_owned())
    })
}

pub(crate) fn read_model_alias_target(ctx_root: &Path, alias: &str) -> std::io::Result<String> {
    let model_dir = open_plain_directory(&ctx_root.join("model"))?;
    let target = nix::fcntl::readlinkat(&model_dir, alias).map_err(std::io::Error::from)?;
    Ok(target.to_string_lossy().into_owned())
}

pub(crate) fn provider_system_secret_path(
    provider: &str,
    account: &str,
) -> Result<PathBuf, ProviderSystemSecretError> {
    if !is_object_name(provider) || !is_secret_account_name(account) {
        return Err(ProviderSystemSecretError::InvalidName);
    }
    Ok(Path::new(PROVIDER_SYSTEM_SECRET_ROOT)
        .join(provider)
        .join(account))
}

pub(crate) fn is_secret_account_name(value: &str) -> bool {
    is_object_name(value)
}
