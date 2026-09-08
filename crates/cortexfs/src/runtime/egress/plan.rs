#![expect(clippy::redundant_pub_crate, reason = "re-exported by egress facade")]

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use super::ProviderEgressError;
use super::secret::ProviderEgressCredential;
use super::secret::provider_egress_credential;
use super::target::{ProviderTarget, insert_target};
use crate::is_object_name;
use crate::object::executor::{
    MAX_RUNNER_CONTROL_BYTES, model_candidates, model_default_base_url, read_small_plain_text_file,
};
use crate::provider::auth::{
    AuthProviderError, Credential, configured_adapter, resolve_credential,
};
use crate::provider::{ProviderConfig, read_configs};

pub(crate) struct ProviderEgressPlan {
    pub(super) run: String,
    pub(super) targets: Vec<ProviderTarget>,
}

impl ProviderEgressPlan {
    pub(crate) fn from_controls(
        ctx_root: &Path,
        model: &str,
        runtime_env: &[(String, String)],
        run: &str,
    ) -> Result<Self, ProviderEgressError> {
        if !is_object_name(run) {
            return Err(ProviderEgressError::InvalidRun);
        }
        let mut targets = plan_targets(ctx_root, model)?;
        let configs = read_configs(Path::new(crate::SYSTEM_PROVIDER_CONFIG_DIR))
            .map_err(|_error| ProviderEgressError::CredentialUnavailable)?;
        bind_credentials(
            &mut targets,
            &configs,
            runtime_env,
            run,
            |target, config| {
                let adapter = configured_adapter(
                    &target.provider,
                    &config.base_url,
                    config.auth_methods(),
                    config.oauth.clone(),
                )
                .ok_or(AuthProviderError::InvalidConfig)?;
                resolve_credential(adapter.as_ref(), config.oauth.as_ref(), &target.profile)
            },
        )?;
        Ok(Self {
            run: run.to_owned(),
            targets,
        })
    }
}

fn bind_credentials(
    targets: &mut [ProviderTarget],
    configs: &[ProviderConfig],
    runtime_env: &[(String, String)],
    run: &str,
    mut resolve: impl FnMut(
        &ProviderTarget,
        &ProviderConfig,
    ) -> Result<Option<Credential>, AuthProviderError>,
) -> Result<(), ProviderEgressError> {
    for target in targets {
        target.credential =
            provider_egress_credential(runtime_env, &target.provider, &target.profile, run)?;
        let Some(config) = configs.iter().find(|config| {
            config.enabled
                && crate::provider_name_from_config(&config.base_url, config.name.as_deref())
                    .as_deref()
                    == Ok(target.provider.as_str())
        }) else {
            continue;
        };
        let mut base = reqwest::Url::parse(&config.base_url)
            .map_err(|_error| ProviderEgressError::InvalidBaseUrl)?;
        validate_url(&base)?;
        let path = base.path().trim_end_matches('/').to_owned();
        base.set_path(&crate::provider::effective_base_url(&path));
        if base.as_str().trim_end_matches('/') != target.base_url {
            return Err(ProviderEgressError::AuthorityConflict);
        }
        let codex = config
            .oauth
            .as_ref()
            .is_some_and(crate::OAuthProviderConfig::is_codex);
        if codex {
            base.set_path(&path);
            base.as_str()
                .trim_end_matches('/')
                .clone_into(&mut target.base_url);
            target.base_path = path;
        }
        if target.credential.is_none() {
            target.credential = resolve(target, config)
                .map_err(|_error| ProviderEgressError::CredentialUnavailable)?
                .map(|credential| ProviderEgressCredential::from_profile(credential, run, codex))
                .transpose()?;
        }
    }
    Ok(())
}

pub(crate) fn is_provider_model(ctx_root: &Path, model: &str) -> Result<bool, ProviderEgressError> {
    let candidates =
        model_candidates(ctx_root, model).map_err(|_error| ProviderEgressError::InvalidModel)?;
    Ok(candidates
        .first()
        .is_some_and(|candidate| candidate.name != "debug/echo"))
}

pub(super) fn plan_targets(
    ctx_root: &Path,
    model: &str,
) -> Result<Vec<ProviderTarget>, ProviderEgressError> {
    let candidates =
        model_candidates(ctx_root, model).map_err(|_error| ProviderEgressError::InvalidModel)?;
    let routes = read_small_plain_text_file(
        &cortexfs_paths::model_route_path(ctx_root),
        MAX_RUNNER_CONTROL_BYTES,
        "provider egress route",
    )
    .ok();
    if let Some(routes) = routes.as_deref() {
        use crate::object::runner::routes::{RouteMatcher, parse_model_transport_route_table};
        let table = parse_model_transport_route_table(routes)
            .map_err(|_error| ProviderEgressError::InvalidModel)?;
        if table.groups.values().any(|group| group.key_slot.is_some())
            && table
                .rules
                .iter()
                .any(|rule| matches!(rule.matcher, RouteMatcher::ProcessName(_)))
        {
            return Err(ProviderEgressError::UnsupportedRoute);
        }
    }
    let mut targets: BTreeMap<String, ProviderTarget> = BTreeMap::new();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let (provider, name) = candidate
            .name
            .split_once('/')
            .ok_or(ProviderEgressError::InvalidModel)?;
        let default = candidate
            .path
            .parent()
            .map(|parent| parent.join(format!("{name}.d/default")))
            .ok_or(ProviderEgressError::MissingControl)?;
        let content = match read_small_plain_text_file(
            &default,
            MAX_RUNNER_CONTROL_BYTES,
            "provider egress control",
        ) {
            Ok(content) => content,
            Err(error) if index > 0 && error.kind() == io::ErrorKind::NotFound => continue,
            Err(_error) => return Err(ProviderEgressError::MissingControl),
        };
        let base_url =
            model_default_base_url(&content).ok_or(ProviderEgressError::InvalidBaseUrl)?;
        let config = crate::object::runner::types::RunnerProviderConfig {
            name: Some(provider.to_owned()),
            base_url: base_url.clone(),
            auth: Vec::new(),
            oauth: None,
            formats: Vec::new(),
        };
        let profile = crate::object::runner::routes::provider_route(
            &config,
            provider,
            name,
            routes.as_deref(),
        )
        .map_err(|_error| ProviderEgressError::InvalidModel)?
        .key_slot
        .unwrap_or_else(|| "default".to_owned());
        let url =
            reqwest::Url::parse(&base_url).map_err(|_error| ProviderEgressError::InvalidBaseUrl)?;
        validate_url(&url)?;
        let authority = url.origin().ascii_serialization();
        let base_path = crate::provider::effective_base_url(url.path().trim_end_matches('/'));
        if base_path.contains(['%', '\\']) {
            return Err(ProviderEgressError::InvalidBaseUrl);
        }
        let mut canonical = url;
        canonical.set_path(&base_path);
        insert_target(
            &mut targets,
            provider,
            profile,
            &canonical,
            authority,
            base_path,
        )?;
    }
    Ok(targets.into_values().collect())
}

fn validate_url(url: &reqwest::Url) -> Result<(), ProviderEgressError> {
    if !matches!(url.scheme(), "http" | "https")
        || url.cannot_be_a_base()
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderEgressError::InvalidBaseUrl);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
