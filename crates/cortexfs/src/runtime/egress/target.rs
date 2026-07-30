use std::collections::BTreeMap;

use super::ProviderEgressError;
use super::secret::ProviderEgressCredential;

#[derive(Eq, PartialEq)]
pub(super) struct ProviderTarget {
    pub(super) provider: String,
    pub(super) base_url: String,
    pub(super) authority: String,
    pub(super) base_path: String,
    pub(super) credential: Option<ProviderEgressCredential>,
}

pub(super) fn insert_target(
    targets: &mut BTreeMap<String, ProviderTarget>,
    provider: &str,
    canonical: &reqwest::Url,
    authority: String,
    base_path: String,
) -> Result<(), ProviderEgressError> {
    if let Some(known) = targets.get(provider) {
        if known.authority != authority || known.base_path != base_path {
            return Err(ProviderEgressError::AuthorityConflict);
        }
        return Ok(());
    }
    targets.insert(
        provider.to_owned(),
        ProviderTarget {
            provider: provider.to_owned(),
            base_url: canonical.to_string().trim_end_matches('/').to_owned(),
            authority,
            base_path,
            credential: None,
        },
    );
    Ok(())
}
