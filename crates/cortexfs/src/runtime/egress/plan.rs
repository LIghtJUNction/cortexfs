use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use super::ProviderEgressError;
use super::secret::provider_egress_credential;
use super::target::{ProviderTarget, insert_target};
use crate::is_object_name;
use crate::object::executor::{
    MAX_RUNNER_CONTROL_BYTES, model_candidates, model_default_base_url, read_small_plain_text_file,
};

pub struct ProviderEgressPlan {
    pub(super) run: String,
    pub(super) targets: Vec<ProviderTarget>,
}

impl ProviderEgressPlan {
    pub fn from_controls(
        ctx_root: &Path,
        model: &str,
        runtime_env: &[(String, String)],
        run: &str,
    ) -> Result<Self, ProviderEgressError> {
        if !is_object_name(run) {
            return Err(ProviderEgressError::InvalidRun);
        }
        let mut targets = plan_targets(ctx_root, model)?;
        for target in &mut targets {
            target.credential = provider_egress_credential(runtime_env, &target.provider, run)?;
        }
        Ok(Self {
            run: run.to_owned(),
            targets,
        })
    }
}

pub fn is_provider_model(ctx_root: &Path, model: &str) -> Result<bool, ProviderEgressError> {
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
        insert_target(&mut targets, provider, &canonical, authority, base_path)?;
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
